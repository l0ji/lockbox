//! The daemon's routes, organized into a single module per API prefix.

pub mod node;
pub mod reader;

use axum::{
    Json, Router,
    extract::{FromRequest, Request},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use lockbox_proto::api::{ApiError, Code};
use lockbox_store::Error as StoreError;
use serde::de::DeserializeOwned;

use crate::AppState;

/// Build the router that serves the local API.
pub fn router(state: AppState) -> Router {
    node::routes()
        .merge(reader::routes())
        .fallback(not_found)
        .with_state(state)
}

/// Route to use when no module has claimed it.
async fn not_found() -> Failure {
    Failure::new(ApiError {
        code: Code::NotFound,
        error: "no such route".to_owned(),
    })
}

/// Type to use for replying to a request with an error.
#[derive(Debug)]
pub struct Failure {
    status: StatusCode,
    error: ApiError,
}

impl Failure {
    /// Create a failure whose status follows its given code.
    pub fn new(error: ApiError) -> Self {
        Self {
            status: status_of(error.code),
            error,
        }
    }

    /// A failure instance for a body which could not be read.
    pub fn unreadable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            error: ApiError::invalid(message),
        }
    }

    /// Report this failure with a status which its code does not imply.
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
}

impl From<ApiError> for Failure {
    fn from(error: ApiError) -> Self {
        Self::new(error)
    }
}

impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        let code = match &error {
            StoreError::NotFound => Code::NotFound,
            StoreError::Conflict(_) => Code::Conflict,
            StoreError::Invalid(_) => Code::Invalid,
            _ => Code::Internal,
        };
        Self::new(ApiError {
            code,
            error: error.to_string(),
        })
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::error!(error = %self.error.error, "route failed");
        }
        (self.status, Json(self.error)).into_response()
    }
}

/// The status code for each kind of failure.
fn status_of(code: Code) -> StatusCode {
    match code {
        Code::NotFound => StatusCode::NOT_FOUND,
        Code::Conflict | Code::Forked => StatusCode::CONFLICT,
        Code::Invalid => StatusCode::BAD_REQUEST,
        Code::Blocked => StatusCode::FORBIDDEN,
        Code::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// JSON body of a request, which reports a body it cannot read as an API error.
pub struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = Failure;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(value)),
            Err(rejection) => Err(Failure::unreadable(rejection.body_text())),
        }
    }
}
