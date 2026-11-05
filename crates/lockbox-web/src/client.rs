//! HTTP client for the daemon's UNIX socket.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, body::Bytes, header};
use hyper_util::client::legacy;
use hyperlocal::{UnixClientExt, UnixConnector};
use serde::de::DeserializeOwned;
use tokio::time::timeout;

/// How long a single request may take before the client gives up.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum reply size accepted from the daemon, in bytes.
pub const MAX_BODY: usize = 4 * 1024 * 1024;

/// A parsed HTTP reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Status code from the status line.
    pub status: u16,
    /// Reply body.
    pub body: Vec<u8>,
}

impl Response {
    /// Whether the status code is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// A client error that can occur when interacting with the daemon.
#[derive(Debug)]
pub enum ClientError {
    /// The UNIX socket could not be reached.
    Connect(legacy::Error),
    /// The reply could not be read.
    Body(Box<dyn std::error::Error + Send + Sync>),
    /// The request could not be built.
    Request(hyper::http::Error),
    /// The daemon did not answer in time.
    Timeout,
    /// The daemon answered with a non-2xx status.
    Status { status: u16, body: String },
    /// The reply body was not valid JSON.
    Parse(serde_json::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(err) => write!(f, "failed to reach daemon: {}", root_cause(err)),
            Self::Body(err) => write!(f, "cannot read daemon reply: {err}"),
            Self::Request(err) => write!(f, "cannot build daemon request: {err}"),
            Self::Timeout => write!(f, "daemon timed out"),
            Self::Status { status, body } => {
                let body = body.trim();
                if body.is_empty() {
                    write!(f, "daemon unexpectedly replied {status}")
                } else {
                    write!(f, "daemon unexpectedly replied {status}: {body}")
                }
            }
            Self::Parse(err) => write!(f, "cannot parse daemon reply: {err}"),
        }
    }
}

/// Deepest cause in an error chain, which is the one that names the problem.
fn root_cause<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> &'a (dyn std::error::Error + 'static) {
    let mut cause = err;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect(err) => Some(err),
            Self::Body(err) => Some(err.as_ref()),
            Self::Request(err) => Some(err),
            Self::Parse(err) => Some(err),
            Self::Timeout | Self::Status { .. } => None,
        }
    }
}

/// Client for the daemon API.
#[derive(Debug, Clone)]
pub struct DaemonClient {
    socket: PathBuf,
    client: legacy::Client<UnixConnector, Full<Bytes>>,
}

impl DaemonClient {
    /// Talk to the daemon listening on `socket`.
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
            client: legacy::Client::unix(),
        }
    }

    /// The socket this client connects to.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// `GET path`, returning the raw reply.
    pub async fn get(&self, path: &str) -> Result<Response, ClientError> {
        match timeout(REQUEST_TIMEOUT, self.send(path)).await {
            Ok(reply) => reply,
            Err(_) => Err(ClientError::Timeout),
        }
    }

    /// `GET path`, parsed from JSON of type `T`.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let response = self.get(path).await?;
        if !response.is_success() {
            return Err(ClientError::Status {
                status: response.status,
                body: String::from_utf8_lossy(&response.body).into_owned(),
            });
        }
        serde_json::from_slice(&response.body).map_err(ClientError::Parse)
    }

    async fn send(&self, path: &str) -> Result<Response, ClientError> {
        let uri: hyper::Uri = hyperlocal::Uri::new(&self.socket, path).into();
        let request = Request::builder()
            .uri(uri)
            .header(header::HOST, "lockboxd")
            .header(header::ACCEPT, "application/json")
            .body(Full::new(Bytes::new()))
            .map_err(ClientError::Request)?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(ClientError::Connect)?;
        let status = response.status().as_u16();
        let body = Limited::new(response.into_body(), MAX_BODY)
            .collect()
            .await
            .map_err(ClientError::Body)?
            .to_bytes()
            .to_vec();

        Ok(Response { status, body })
    }
}
