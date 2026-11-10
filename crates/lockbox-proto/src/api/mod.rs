//! The API served by the UNIX socket.
//!
//! Implementations and their clients compile against these types, so a change
//! here is a modification to the core protocol. Bump [`crate::PROTOCOL_VERSION`]
//! when a client built against an older version is expected to have *extreme*
//! technical difficulties with any changes.
//!
//! The routes are grouped by their prefix, and each group keeps the types it
//! exchanges beside it: [`node`] covers the running node.

pub mod cursor;
pub mod node;

use serde::{Deserialize, Serialize};

pub use cursor::Cursor;

/// One page of a list route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// Rows in this page.
    pub items: Vec<T>,
    /// Cursor for items that may appear after the current page. This string is
    /// null if the list has been exhausted.
    pub next: Option<String>,
}

/// Basic API-side error type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// The failure code that a client can match on.
    pub code: Code,
    /// Human readable reason.
    pub error: String,
}

impl ApiError {
    /// Error for an input the route cannot accept.
    pub fn invalid(error: impl Into<String>) -> Self {
        Self {
            code: Code::Invalid,
            error: error.into(),
        }
    }

    /// Error for a failure which is the fault of the protocol.
    pub fn internal(error: impl Into<String>) -> Self {
        Self {
            code: Code::Internal,
            error: error.into(),
        }
    }
}

/// Kinds of failure that a route can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    /// The requested item does not exist.
    NotFound,
    /// The request clashes with the current state of the item.
    Conflict,
    /// The request cannot be understood or accepted.
    Invalid,
    /// The author is blocked, or the peer refuses to serve.
    Blocked,
    /// Another branch of the account won, so the account state cannot be served.
    Forked,
    /// The serveer failed to handle the request.
    Internal,
}
