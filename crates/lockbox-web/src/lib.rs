//! Common code for interfacing between the web server and the great beyond.

mod bookmarks;
mod client;
mod data;
mod page;
mod reader;

use std::{path::Path, sync::Arc};

use askama::Template;
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response as HttpResponse},
    routing::get,
};
use lockbox_proto::api::{ApiError, Code, node::STATUS_PATH};
use lockbox_types::node::Status;

use client::Response as Reply;
pub use client::{ClientError, DaemonClient, Response};
pub use data::{Choice, NavLink};
pub use page::{ErrorPage, NODE_PAGE, Page};
pub use reader::{
    EntriesPage, EntryPage, FeedRow, FeedsPage, FilterLink, Row, TEXT_LIMIT, truncate,
};

/// Shared state for the web client's routes.
#[derive(Clone)]
pub struct AppState {
    client: Arc<DaemonClient>,
}

impl AppState {
    /// Spawn a new [`AppState`].
    pub fn new(client: DaemonClient) -> Self {
        Self {
            client: Arc::new(client),
        }
    }

    /// The daemon currently in use.
    pub fn client(&self) -> &DaemonClient {
        &self.client
    }
}

/// Embedded `static/` directory.
mod assets {
    include!(concat!(env!("OUT_DIR"), "/static.rs"));
}

/// Build the client's router.
///
/// Paths the routes do not claim are treated as files from `static/`.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(NODE_PAGE, get(index))
        .route(STATUS_PATH, get(status))
        .merge(reader::routes())
        .merge(bookmarks::routes())
        .with_state(state)
        .fallback(asset)
}

/// Serve a file from `static/`, keyed by the request path.
async fn asset(uri: Uri) -> HttpResponse {
    let requested = uri.path().trim_start_matches('/');
    match assets::ASSETS.iter().find(|(path, _)| *path == requested) {
        Some((path, body)) => ([(header::CONTENT_TYPE, content_type(path))], *body).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Content type for a static file, guessed from its extension.
/// Ideally should be replaced by a proper library.
fn content_type(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|ext| ext.to_str()) {
        Some("css") => "text/css; charset=utf-8",
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// Render the status page, with the error line filled in when the daemon is down.
async fn index(State(state): State<AppState>) -> Result<Html<String>, StatusCode> {
    let page = match state.client.get_json::<Status>(STATUS_PATH).await {
        Ok(status) => Page::live(status),
        Err(err) => {
            tracing::warn!("{err}");
            Page::down(err.to_string())
        }
    };

    match page.render() {
        Ok(html) => Ok(Html(html)),
        Err(err) => {
            tracing::error!("{err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Get the daemon's current status.
///
/// A dead or unreachable daemon is a gateway failure from the browser's point
/// of view, so it maps to 502 with the reason in the body. Maybe find a better
/// code?
async fn status(
    State(state): State<AppState>,
) -> Result<Json<Status>, (StatusCode, Json<ApiError>)> {
    match state.client.get_json::<Status>(STATUS_PATH).await {
        Ok(status) => Ok(Json(status)),
        Err(err) => {
            tracing::warn!("{err}");
            Err((
                StatusCode::BAD_GATEWAY,
                Json(ApiError {
                    code: Code::Internal,
                    error: err.to_string(),
                }),
            ))
        }
    }
}

/// Renders a page or returns a server error.
pub(crate) fn render<T: Template>(page: T) -> Result<Html<String>, Failure> {
    match page.render() {
        Ok(body) => Ok(Html(body)),
        Err(error) => {
            tracing::error!("{error}");
            Err(Failure {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
                offline: false,
            })
        }
    }
}

/// Reads the daemon error message, falling back to the status code.
pub(crate) fn reason(reply: &Reply) -> String {
    match serde_json::from_slice::<ApiError>(&reply.body) {
        Ok(error) => error.error,
        Err(_) => format!("daemon replied {}", reply.status),
    }
}

/// Accepts a local return path or uses the fallback.
pub(crate) fn return_to(back: Option<String>, fallback: &str) -> String {
    match back {
        Some(back) if back.starts_with('/') && !back.starts_with("//") => back,
        _ => fallback.to_owned(),
    }
}

/// A failed request, shown by the error page.
#[derive(Debug)]
pub(crate) struct Failure {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
    pub(crate) offline: bool,
}

impl Failure {
    pub(crate) fn from_reply(reply: &Reply) -> Self {
        Self {
            status: StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY),
            message: reason(reply),
            offline: false,
        }
    }
}

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Self {
        match &error {
            ClientError::Offline => Self {
                status: StatusCode::BAD_GATEWAY,
                message: error.to_string(),
                offline: true,
            },
            ClientError::Status { status, body } => Self {
                status: StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
                message: match serde_json::from_str::<ApiError>(body) {
                    Ok(named) => named.error,
                    Err(_) => error.to_string(),
                },
                offline: false,
            },
            _ => Self {
                status: StatusCode::BAD_GATEWAY,
                message: error.to_string(),
                offline: false,
            },
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> HttpResponse {
        if self.status.is_server_error() {
            tracing::warn!(status = %self.status, "{message}", message = self.message);
        }
        let page = ErrorPage {
            section: "",
            status: self.status.as_u16(),
            message: self.message,
            offline: self.offline,
        };
        match page.render() {
            Ok(html) => (self.status, Html(html)).into_response(),
            Err(error) => {
                tracing::error!("{error}");
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        }
    }
}
