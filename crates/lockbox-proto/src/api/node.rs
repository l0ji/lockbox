//! Routes meant to be used for a common C2S API.

use serde::{Deserialize, Serialize};

/// Route that returns [`Health`].
pub const HEALTH_PATH: &str = "/api/node/health";

/// Route that returns [`Status`].
pub const STATUS_PATH: &str = "/api/node/status";

/// Snapshot of the node's information, as reported to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Core protocol version the node speaks (automatically set to
    /// [`crate::PROTOCOL_VERSION`]).
    pub protocol_version: u16,
    /// Version of the software that runs the node.
    pub software_version: String,
    /// Public key of the node.
    pub node_id: String,
    /// Relay URLs that the node currently uses.
    pub relay_urls: Vec<String>,
    /// Direct socket addresses that the node currently advertises.
    pub direct_addrs: Vec<String>,
    /// Time in seconds since the node began binding to the network.
    pub uptime_secs: u64,
}

/// Basic health check type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub ok: bool,
}
