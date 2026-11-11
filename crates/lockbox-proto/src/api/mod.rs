//! The API served by the UNIX socket.
//!
//! Implementations and their clients compile against these types, so a change
//! here is a modification to the core protocol. Bump [`crate::PROTOCOL_VERSION`]
//! when a client built against an older version is expected to have *extreme*
//! technical difficulties with any changes.
//!
//! The routes are grouped by their prefix, and each group keeps the types it
//! exchanges beside it: [`node`] covers the running node and [`reader`] the
//! feeds the user follows.

pub mod cursor;
pub mod node;
pub mod reader;

use serde::{Deserialize, Serialize};

pub use cursor::Cursor;

/// How many items a list route returns when the client does not say.
pub const DEFAULT_LIMIT: usize = 50;

/// Query of a list route which only pages through one order of one list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListQuery {
    /// How many items the page holds.
    pub limit: Option<usize>,
    /// Cursor which the previous page answered with.
    pub cursor: Option<String>,
}

impl ListQuery {
    /// This query as a request string, which starts with `?` when it holds
    /// anything at all.
    pub fn to_query(&self) -> String {
        query_string(&[
            ("limit", self.limit.map(|limit| limit.to_string())),
            ("cursor", self.cursor.clone()),
        ])
    }
}

/// Read a field which may be left out, or sent as a `null` to clear it.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// Write the pairs which hold a value as a request string.
fn query_string(pairs: &[(&str, Option<String>)]) -> String {
    let mut query = String::new();
    for (name, value) in pairs {
        let Some(value) = value else { continue };
        query.push(if query.is_empty() { '?' } else { '&' });
        query.push_str(name);
        query.push('=');
        query.push_str(&escape(value));
    }
    query
}

/// Escape everything a request string may not hold.
fn escape(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(byte as char)
            }
            _ => {
                escaped.push('%');
                escaped.push(HEX[usize::from(byte >> 4)] as char);
                escaped.push(HEX[usize::from(byte & 0xf)] as char);
            }
        }
    }
    escaped
}

/// One page of a list route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// Rows in this page.
    pub items: Vec<T>,
    /// Cursor of the next page, or `null` when the list ends here.
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
    /// The server failed to handle the request.
    Internal,
}
