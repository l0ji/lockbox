//! What a node reports about itself.

use serde::{Deserialize, Serialize};

/// Snapshot of the node's information, as reported to clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// Core protocol version the node speaks.
    pub protocol_version: u16,
    /// Version of the software that runs the node.
    pub software_version: String,
    /// Public key of the node's account.
    pub public_key: String,
    /// Public key of the node's account, as hex.
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
