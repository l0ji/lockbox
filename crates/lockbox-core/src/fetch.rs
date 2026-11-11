//! HTTP client for the documents fetched by the reader.

use std::{fmt, time::Duration};

use reqwest::{
    Client, Response, StatusCode,
    header::{ACCEPT, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, USER_AGENT},
};
use url::Url;

/// Largest document body accepted, in bytes.
pub const MAX_DOCUMENT: usize = 4 * 1024 * 1024;

/// What the server accepted as document types.
const ACCEPTED: &str = "application/rss+xml, application/atom+xml, application/feed+json, \
     application/json;q=0.9, text/html;q=0.8, */*;q=0.5";

/// How one document is fetched.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// Address to fetch.
    pub url: &'a str,
    /// Validator from the last fetch, which the server may match.
    pub etag: Option<&'a str>,
    /// Date from the last fetch, which the server may match.
    pub last_modified: Option<&'a str>,
    /// How long the server has to answer.
    pub timeout: Duration,
    /// User agent the request sends.
    pub user_agent: &'a str,
}

/// What the result of a fetch for one address found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// Address the document came from, after any redirects.
    pub url: Url,
    /// Body of the document, which is empty when nothing changed.
    pub body: Vec<u8>,
    /// Validator to send on the next fetch of the same address.
    pub etag: Option<String>,
    /// Date to send on the next fetch of the same address.
    pub last_modified: Option<String>,
    /// Whether the server says we already have this document.
    pub unchanged: bool,
}

impl Fetched {
    /// Body of the document as text, with broken bytes replaced.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// HTTP client which fetches one document per call.
#[derive(Debug, Clone)]
pub struct Fetcher {
    client: Client,
}

impl Fetcher {
    /// Build an HTTP client which every fetch reuses.
    pub fn new() -> Result<Self, Error> {
        let client = Client::builder().build().map_err(Error::Request)?;
        Ok(Self { client })
    }

    /// Fetch one document, asking for the body only when it changed.
    ///
    /// The validators are what the last fetch of the same address left behind,
    /// and they are left out of the request when they are missing.
    pub async fn fetch(&self, request: &Request<'_>) -> Result<Fetched, Error> {
        let target = Url::parse(request.url).map_err(Error::Url)?;
        let mut sent = self
            .client
            .get(target.clone())
            .header(ACCEPT, ACCEPTED)
            .header(USER_AGENT, request.user_agent)
            .timeout(request.timeout);
        if let Some(etag) = request.etag {
            sent = sent.header(IF_NONE_MATCH, etag);
        }
        if let Some(date) = request.last_modified {
            sent = sent.header(IF_MODIFIED_SINCE, date);
        }

        let response = sent.send().await.map_err(Error::Request)?;
        let url = Url::parse(response.url().as_str()).unwrap_or(target);
        let unchanged = response.status() == StatusCode::NOT_MODIFIED;
        if !unchanged && !response.status().is_success() {
            return Err(Error::Status(response.status()));
        }

        let (etag, last_modified) =
            validators(&response, unchanged, request.etag, request.last_modified);
        let body = if unchanged {
            Vec::new()
        } else {
            read(response).await?
        };
        Ok(Fetched {
            url,
            body,
            etag,
            last_modified,
            unchanged,
        })
    }
}

/// The validators that are attached to a reply, used when a document is new or
/// modified.
fn validators(
    response: &Response,
    unchanged: bool,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> (Option<String>, Option<String>) {
    let stored = |sent: Option<&str>, name| {
        if unchanged {
            sent.map(str::to_owned).or_else(|| header(response, name))
        } else {
            header(response, name)
        }
    };
    (stored(etag, ETAG), stored(last_modified, LAST_MODIFIED))
}

/// Read a single header from a response as text.
fn header(response: &Response, name: reqwest::header::HeaderName) -> Option<String> {
    response
        .headers()
        .get(name)?
        .to_str()
        .ok()
        .map(str::to_owned)
}

/// Read a response body, refusing one which is too large.
async fn read(mut response: Response) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(Error::Request)? {
        if body.len() + chunk.len() > MAX_DOCUMENT {
            return Err(Error::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Why a fetch did not produce a document.
#[derive(Debug)]
pub enum Error {
    /// The address is not a valid URL.
    Url(url::ParseError),
    /// The request failed or timed out.
    Request(reqwest::Error),
    /// The server answered, but not with a document.
    Status(StatusCode),
    /// The document was larger than [`MAX_DOCUMENT`].
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Url(error) => write!(f, "not a URL: {error}"),
            Self::Request(error) => write!(f, "{}", cause(error)),
            Self::Status(status) => write!(f, "server replied {status}"),
            Self::TooLarge => write!(f, "document is larger than {MAX_DOCUMENT} bytes"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Url(error) => Some(error),
            Self::Request(error) => Some(error),
            Self::Status(_) | Self::TooLarge => None,
        }
    }
}

/// Deepest cause in an error chain, which is the one that names the problem.
fn cause<'a>(
    error: &'a (dyn std::error::Error + 'static),
) -> &'a (dyn std::error::Error + 'static) {
    let mut cause = error;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause
}
