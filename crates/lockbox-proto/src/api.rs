//! The API served by the UNIX socket.
//!
//! Both the daemon and any clients compile against these types, so a change
//! here is a modification to the core protocol. Bump [`crate::PROTOCOL_VERSION`]
//! when a client built against an older version is expected to have *extreme*
//! technical difficulties with any changes.

use serde::{Deserialize, Serialize};

/// Route that returns [`Health`].
pub const HEALTH_PATH: &str = "/api/health";

/// Route that returns [`Status`].
pub const STATUS_PATH: &str = "/api/status";

/// Snapshot of the daemon's node, as reported to local clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Core protocol version the daemon speaks ([`crate::PROTOCOL_VERSION`]).
    pub protocol_version: u16,
    /// Version of the currently connected daemon.
    pub daemon_version: String,
    /// Public key of the node.
    pub node_id: String,
    /// Relay URLs the node currently advertises.
    pub relay_urls: Vec<String>,
    /// Direct socket addresses the node currently advertises.
    pub direct_addrs: Vec<String>,
    /// Time in seconds since the daemon bound its node.
    pub uptime_secs: u64,
}

/// Basic health check type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
}

/// Basic API-side error type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiError {
    /// Human readable reason.
    pub error: String,
}
