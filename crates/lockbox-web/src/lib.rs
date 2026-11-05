//! Common code for interfacing between the web server and the great beyond.

mod client;
mod page;

use std::sync::Arc;

use askama::Template;
use axum::{Json, Router, extract::State, http::StatusCode, response::Html, routing::get};
use lockbox_proto::api::{ApiError, STATUS_PATH, Status};

pub use client::{ClientError, DaemonClient, Response};
use page::Page;

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

/// Build the client's router.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route(STATUS_PATH, get(status))
        .with_state(state)
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
                    error: err.to_string(),
                }),
            ))
        }
    }
}
