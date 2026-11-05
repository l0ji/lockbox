//! End-to-end tests of the web client against a stub daemon's UNIX socket.
//!
//! The stub speaks the same routes as the real daemon but has no iroh node, so
//! this tests the socket client, the proxy handler, and the page without
//! touching the network.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    routing::get,
};
use lockbox_proto::api::{ApiError, STATUS_PATH, Status};
use lockbox_web::{AppState, DaemonClient, router};
use tower::ServiceExt;

fn sample_status() -> Status {
    Status {
        protocol_version: 1,
        daemon_version: "0.1.0".to_owned(),
        node_id: "test-node".to_owned(),
        relay_urls: vec!["https://relay.example./".to_owned()],
        direct_addrs: vec!["127.0.0.1:5555".to_owned()],
        uptime_secs: 42,
    }
}

/// A stub daemon listening on a socket inside a throwaway directory.
struct Stub {
    dir: PathBuf,
    socket: PathBuf,
}

impl Stub {
    async fn start(status: Status) -> Self {
        let app = Router::new().route(
            STATUS_PATH,
            get(move || {
                let status = status.clone();
                async move { Json(status) }
            }),
        );
        Self::serve(app).await
    }

    async fn serve(app: Router) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lockbox-web-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let socket = dir.join("daemon.sock");

        let listener = tokio::net::UnixListener::bind(&socket).expect("bind test socket");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { dir, socket }
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn request(path: &str, socket: PathBuf) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("build request");
    let response = router(AppState::new(DaemonClient::new(socket)))
        .oneshot(request)
        .await
        .expect("web client must not fail");
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read body");
    (status, body.to_vec())
}

#[tokio::test]
async fn proxies_status_from_daemon() {
    let status = sample_status();
    let stub = Stub::start(status.clone()).await;

    let (code, body) = request(STATUS_PATH, stub.socket.clone()).await;
    assert_eq!(code, StatusCode::OK);
    let proxied: Status = serde_json::from_slice(&body).expect("status json");
    assert_eq!(proxied, status);
}

#[tokio::test]
async fn reports_dead_daemon_as_bad_gateway() {
    let missing = std::env::temp_dir().join("lockbox-web-test-missing.sock");
    let _ = std::fs::remove_file(&missing);

    let (code, body) = request(STATUS_PATH, missing.clone()).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
    let error: ApiError = serde_json::from_slice(&body).expect("error json");
    assert!(error.error.contains("failed to reach daemon"), "{error:?}");

    let (code, body) = request("/", missing).await;
    assert_eq!(code, StatusCode::OK);
    let html = String::from_utf8(body).expect("utf8 page");
    assert!(html.contains("daemon unreachable"), "{html}");
    assert!(html.contains("data-state=\"down\""), "{html}");
}

#[tokio::test]
async fn reports_daemon_error_as_bad_gateway() {
    let app = Router::new().route(
        STATUS_PATH,
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    error: "node is on fire".to_owned(),
                }),
            )
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, body) = request(STATUS_PATH, stub.socket.clone()).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
    let error: ApiError = serde_json::from_slice(&body).expect("error json");
    assert!(
        error.error.contains("daemon unexpectedly replied 500"),
        "{error:?}"
    );
}

#[tokio::test]
async fn serves_the_page() {
    let stub = Stub::start(sample_status()).await;

    let (code, body) = request("/", stub.socket.clone()).await;
    assert_eq!(code, StatusCode::OK);
    let html = String::from_utf8(body).expect("utf8 page");
    assert!(html.contains("<title>lockbox</title>"), "{html}");
    assert!(html.contains("data-state=\"live\""), "{html}");
    assert!(html.contains("test-node"), "{html}");
    assert!(html.contains("42s"), "{html}");
    assert!(html.contains("https://relay.example./"), "{html}");
}
