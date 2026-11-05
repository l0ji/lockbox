//! The lockbox node, which handles the server side of the p2p protocol.
//!
//! A [`Node`] owns one iroh endpoint. The daemon keeps a node alive for
//! its lifetime and turns the protocol into a local API meant to be consumed
//! by clients.

use std::time::Instant;

use iroh::{Endpoint, endpoint::presets};
pub use iroh::{EndpointAddr, EndpointId, SecretKey};
use lockbox_proto::{PROTOCOL_VERSION, api::Status};

/// Error type used across the node's code.
///
/// Temporary: replace with a proper error enum once the protocol grows.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A live lockbox node.
pub struct Node {
    endpoint: Endpoint,
    started: Instant,
}

impl Node {
    /// Bind an endpoint on the default n0 relay network with the lockbox ALPN.
    ///
    /// The node authenticates as `secret`, which is the account keypair the
    /// daemon owns. The node will immediately start and does not need to wait
    /// to connect to the relay to become operational.
    pub async fn bind(secret: SecretKey) -> Result<Self, BoxError> {
        let endpoint = Endpoint::builder(presets::N0)
            .alpns(vec![lockbox_proto::ALPN.to_vec()])
            .secret_key(secret)
            .bind()
            .await?;
        Ok(Self {
            endpoint,
            started: Instant::now(),
        })
    }

    /// Public key that identifies this node.
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Known addresses for this node, typically the relay plus any direct
    /// addresses.
    pub fn addr(&self) -> EndpointAddr {
        self.endpoint.addr()
    }

    /// Time in seconds since this node was bound.
    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// Wait until at least one relay connection is ready.
    pub async fn online(&self) {
        self.endpoint.online().await;
    }

    /// Close the endpoint.
    pub async fn shutdown(&self) {
        self.endpoint.close().await;
    }

    /// Current information of the node, formatted to match the API's schema.
    pub fn status(&self) -> Status {
        let addr = self.addr();
        Status {
            protocol_version: PROTOCOL_VERSION,
            daemon_version: env!("CARGO_PKG_VERSION").to_owned(),
            node_id: addr.id.to_string(),
            relay_urls: addr.relay_urls().map(|url| url.to_string()).collect(),
            direct_addrs: addr.ip_addrs().map(|addr| addr.to_string()).collect(),
            uptime_secs: self.uptime_secs(),
        }
    }
}
