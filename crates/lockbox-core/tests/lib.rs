//! Tests of the node's public surface.

use std::{net::SocketAddr, time::Duration};

use lockbox_core::{Node, SecretKey};
use lockbox_proto::{PROTOCOL_VERSION, key};

#[tokio::test]
async fn reports_status_of_the_bound_node() {
    let node = Node::bind(SecretKey::generate())
        .await
        .expect("node must bind");
    let status = node.status();

    assert_eq!(status.protocol_version, PROTOCOL_VERSION);
    assert_eq!(status.software_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.node_id, node.id().to_string());
    assert_eq!(
        key::decode_public(&status.public_key).expect("encoded public key"),
        *node.id().as_bytes(),
        "the encoded key is the identity the hex names"
    );
    assert!(
        status
            .relay_urls
            .iter()
            .all(|url| url.starts_with("https://")),
        "{:?}",
        status.relay_urls
    );
    assert!(
        status
            .direct_addrs
            .iter()
            .all(|addr| addr.parse::<SocketAddr>().is_ok()),
        "{:?}",
        status.direct_addrs
    );

    node.shutdown().await;
}

#[tokio::test]
async fn counts_uptime_from_binding() {
    let node = Node::bind(SecretKey::generate())
        .await
        .expect("node must bind");
    let bound = node.uptime_secs();

    tokio::time::sleep(Duration::from_millis(1100)).await;
    assert!(node.uptime_secs() > bound, "{bound}");

    node.shutdown().await;
}

#[tokio::test]
async fn keeps_the_identity_of_its_secret_key() {
    let secret = SecretKey::from_bytes(&[7; 32]);
    let public = secret.public();
    let node = Node::bind(secret).await.expect("node must bind");
    assert_eq!(node.id(), public);
    assert_eq!(node.status().node_id, public.to_string());

    node.shutdown().await;
}
