//! HTTP client for the daemon's UNIX socket.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use http_body_util::{BodyExt, Full, Limited};
use hyper::{Method, Request, body::Bytes, header};
use hyper_util::client::legacy;
use hyperlocal::{UnixClientExt, UnixConnector};
use lockbox_types::root_cause;
use serde::{Serialize, de::DeserializeOwned};
use tokio::time::timeout;

/// How long a single request may take before the client gives up.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Maximum reply size accepted from the daemon, in bytes.
pub const MAX_REPLY: usize = 4 * 1024 * 1024;

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

    /// Read the body as `T`, reporting a reply which is not a success.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, ClientError> {
        if !self.is_success() {
            return Err(ClientError::Status {
                status: self.status,
                body: String::from_utf8_lossy(&self.body).into_owned(),
            });
        }
        serde_json::from_slice(&self.body).map_err(ClientError::Parse)
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
    /// The daemon's socket is gone, so it is presumably offline.
    Offline,
    /// The daemon answered with a non-2xx status.
    Status { status: u16, body: String },
    /// The request or the reply was not valid JSON.
    Parse(serde_json::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connect(err) => write!(f, "failed to reach daemon: {}", root_cause(err)),
            Self::Body(err) => write!(f, "cannot read daemon reply: {err}"),
            Self::Request(err) => write!(f, "cannot build daemon request: {err}"),
            Self::Timeout => write!(f, "daemon timed out"),
            Self::Offline => write!(f, "the daemon is offline"),
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

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Connect(err) => Some(err),
            Self::Body(err) => Some(err.as_ref()),
            Self::Request(err) => Some(err),
            Self::Parse(err) => Some(err),
            Self::Timeout | Self::Status { .. } | Self::Offline => None,
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
        self.request(Method::GET, path, None).await
    }

    /// `GET path`, parsed from JSON of type `T`.
    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.get(path).await?.json()
    }

    /// `POST path` with a JSON body, returning the raw reply.
    pub async fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<Response, ClientError> {
        let body = serde_json::to_string(body).map_err(ClientError::Parse)?;
        self.request(Method::POST, path, Some(body)).await
    }

    /// `PATCH path` with a JSON body, returning the raw reply.
    pub async fn patch<T: Serialize>(&self, path: &str, body: &T) -> Result<Response, ClientError> {
        let body = serde_json::to_string(body).map_err(ClientError::Parse)?;
        self.request(Method::PATCH, path, Some(body)).await
    }

    /// `DELETE path`, returning the raw reply.
    pub async fn delete(&self, path: &str) -> Result<Response, ClientError> {
        self.request(Method::DELETE, path, None).await
    }

    /// Send one request, giving up when the daemon does not answer in time.
    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
    ) -> Result<Response, ClientError> {
        match timeout(REQUEST_TIMEOUT, self.send(method, path, body)).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(error)) => Err(self.offline(error)),
            Err(_) => Err(ClientError::Timeout),
        }
    }

    /// Report a missing socket as an offline daemon.
    fn offline(&self, error: ClientError) -> ClientError {
        match error {
            ClientError::Connect(_) if !self.socket.exists() => ClientError::Offline,
            other => other,
        }
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<String>,
    ) -> Result<Response, ClientError> {
        let uri: hyper::Uri = hyperlocal::Uri::new(&self.socket, path).into();
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "lockboxd")
            .header(header::ACCEPT, "application/json");

        let body = match body {
            Some(body) => {
                request = request.header(header::CONTENT_TYPE, "application/json");
                Full::new(Bytes::from(body))
            }
            None => Full::new(Bytes::new()),
        };
        let request = request.body(body).map_err(ClientError::Request)?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(ClientError::Connect)?;
        let status = response.status().as_u16();
        let body = Limited::new(response.into_body(), MAX_REPLY)
            .collect()
            .await
            .map_err(ClientError::Body)?
            .to_bytes()
            .to_vec();

        Ok(Response { status, body })
    }
}
