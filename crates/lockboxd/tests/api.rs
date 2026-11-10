//! Tests of the local API's node routes.

mod common;

use std::collections::HashMap;

use common::{Daemon, json, request};
use lockbox_core::settings::{PATH as SETTINGS_PATH, Settings};
use lockbox_proto::{
    PROTOCOL_VERSION,
    api::{
        ApiError, Code,
        node::{self, Health, Status},
    },
};
use lockbox_store::local::settings;

/// Read the settings over the API.
async fn fetch(daemon: &Daemon) -> Settings {
    let (code, body) = request(daemon.socket(), "GET", SETTINGS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

/// Change settings over the API.
async fn patch(daemon: &Daemon, change: &str) -> Settings {
    let (code, body) = request(daemon.socket(), "PATCH", SETTINGS_PATH, Some(change)).await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

#[tokio::test]
async fn answers_a_health_check() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "GET", node::HEALTH_PATH, None).await;

    assert_eq!(code, 200, "{body}");
    let health: Health = json(&body);
    assert!(health.ok);
}

#[tokio::test]
async fn reports_the_node() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "GET", node::STATUS_PATH, None).await;

    assert_eq!(code, 200, "{body}");
    let status: Status = json(&body);
    assert_eq!(status.protocol_version, PROTOCOL_VERSION);
    assert_eq!(status.software_version, env!("CARGO_PKG_VERSION"));
    assert!(!status.node_id.is_empty());

    let (_, again) = request(daemon.socket(), "GET", node::STATUS_PATH, None).await;
    let again: Status = json(&again);
    assert_eq!(again.node_id, status.node_id);
}

#[tokio::test]
async fn reports_routes_which_do_not_exist() {
    let daemon = Daemon::start().await;

    for path in ["/api/health", "/api/status", "/api/node"] {
        let (code, body) = request(daemon.socket(), "GET", path, None).await;
        assert_eq!(code, 404, "{path}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound, "{path}");
    }
}

#[tokio::test]
async fn serves_the_default_settings() {
    let daemon = Daemon::start().await;
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn changes_one_setting() {
    let daemon = Daemon::start().await;

    let expected = Settings {
        trust_depth: 3,
        ..Settings::default()
    };
    let changed = patch(&daemon, r#"{"trust_depth":3}"#).await;
    assert_eq!(changed, expected);
    assert_eq!(fetch(&daemon).await, expected);

    // Only the value which changed is written down
    let stored = daemon
        .db()
        .call(|connection| settings::load(connection))
        .await
        .expect("read the settings table");
    assert_eq!(
        stored,
        HashMap::from([("trust_depth".to_owned(), "3".to_owned())])
    );
}

#[tokio::test]
async fn keeps_the_settings_when_nothing_is_named() {
    let daemon = Daemon::start().await;

    patch(&daemon, "{}").await;
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn clears_a_setting_sent_as_null() {
    let daemon = Daemon::start().await;

    let set = patch(&daemon, r#"{"retention_days":30}"#).await;
    assert_eq!(set.retention_days, Some(30));

    let cleared = patch(&daemon, r#"{"retention_days":null}"#).await;
    assert_eq!(cleared.retention_days, None);
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_setting_it_does_not_know() {
    let daemon = Daemon::start().await;
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        SETTINGS_PATH,
        Some(r#"{"trust":3}"#),
    )
    .await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
    assert!(error.error.contains("trust"), "{error:?}");
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_setting_of_the_wrong_type() {
    let daemon = Daemon::start().await;
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        SETTINGS_PATH,
        Some(r#"{"trust_depth":"two"}"#),
    )
    .await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_body_which_is_not_json() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "PATCH", SETTINGS_PATH, Some("{")).await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}
