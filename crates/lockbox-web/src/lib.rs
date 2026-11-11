//! Common code for interfacing between the web server and the great beyond.

mod client;
mod page;
mod reader;

use std::{path::Path, sync::Arc};

use askama::Template;
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, Uri, header},
    response::{self, Html, IntoResponse},
    routing::get,
};
use lockbox_proto::api::{ApiError, Code, node::STATUS_PATH};
use lockbox_types::node::Status;

pub use client::{ClientError, DaemonClient, Response};
pub use page::{ErrorPage, NODE_PAGE, Page};
pub use reader::{
    EntriesPage, EntryPage, FeedLink, FeedRow, FeedsPage, FilterLink, Row, TEXT_LIMIT, truncate,
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
        .with_state(state)
        .fallback(asset)
}

/// Serve a file from `static/`, keyed by the request path.
async fn asset(uri: Uri) -> response::Response {
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
