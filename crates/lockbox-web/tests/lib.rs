//! Tests of the routes created by [`router`].
//!
//! The stubs speak the same routes as the real daemon but have no iroh node, so
//! these run the handlers without touching the network.

mod common;

use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode, header},
    routing::get,
};
use common::{Stub, sample_status};
use lockbox_proto::api::{
    ApiError, Code, Page,
    node::{STATUS_PATH, Status},
    reader::{self, EntrySummary, Feed},
};
use lockbox_web::{AppState, DaemonClient, NODE_PAGE, router};
use tower::ServiceExt;

/// One request against a fresh router, returned as (status, body).
async fn request(path: &str, socket: impl Into<std::path::PathBuf>) -> (StatusCode, Vec<u8>) {
    let request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("build request");
    let response = router(AppState::new(DaemonClient::new(socket.into())))
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
async fn serves_the_node_page() {
    let stub = Stub::start(sample_status()).await;

    let (code, body) = request(NODE_PAGE, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let html = String::from_utf8(body).expect("utf8 page");
    assert!(html.contains("<title>lockbox: node</title>"), "{html}");
    assert!(html.contains("data-state=\"live\""), "{html}");
    // The navbar is on this page too, with its own section marked
    assert!(
        html.contains(r#"<a class="tab on" href="/node">node</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/">reader</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/feeds">feeds</a>"#),
        "{html}"
    );
    assert!(html.contains("<td>test-node</td>"), "{html}");
}

#[tokio::test]
async fn serves_the_node_page_when_the_daemon_is_down() {
    let missing = std::env::temp_dir().join("lockbox-web-test-missing.sock");
    let _ = std::fs::remove_file(&missing);

    let (code, body) = request(NODE_PAGE, missing).await;
    assert_eq!(code, StatusCode::OK);
    let html = String::from_utf8(body).expect("utf8 page");
    assert!(html.contains("daemon offline"), "{html}");
    assert!(html.contains("data-state=\"down\""), "{html}");
}

#[tokio::test]
async fn serves_the_reader_at_the_root() {
    let app = Router::new()
        .route(
            reader::FEEDS_PATH,
            get(|| async {
                Json(Page {
                    items: Vec::<Feed>::new(),
                    next: None,
                })
            }),
        )
        .route(
            reader::ENTRIES_PATH,
            get(|| async {
                Json(Page {
                    items: Vec::<EntrySummary>::new(),
                    next: None,
                })
            }),
        );
    let stub = Stub::serve(app).await;

    let (code, body) = request("/", stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let html = String::from_utf8(body).expect("utf8 page");
    assert!(html.contains("<title>lockbox: reader</title>"), "{html}");
    assert!(
        html.contains(r#"<a class="tab on" href="/">reader</a>"#),
        "{html}"
    );
    assert!(html.contains("nothing to read yet"), "{html}");
}

#[tokio::test]
async fn proxies_status_from_daemon() {
    let status = sample_status();
    let stub = Stub::start(status.clone()).await;

    let (code, body) = request(STATUS_PATH, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let proxied: Status = serde_json::from_slice(&body).expect("status json");
    assert_eq!(proxied, status);
}

#[tokio::test]
async fn reports_dead_daemon_as_bad_gateway() {
    let missing = std::env::temp_dir().join("lockbox-web-test-missing.sock");
    let _ = std::fs::remove_file(&missing);

    let (code, body) = request(STATUS_PATH, missing).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
    let error: ApiError = serde_json::from_slice(&body).expect("error json");
    assert!(error.error.contains("the daemon is offline"), "{error:?}");
}

#[tokio::test]
async fn reports_daemon_error_as_bad_gateway() {
    let app = Router::new().route(
        STATUS_PATH,
        get(|| async {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: Code::Internal,
                    error: "node is on fire".to_owned(),
                }),
            )
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, body) = request(STATUS_PATH, stub.socket()).await;
    assert_eq!(code, StatusCode::BAD_GATEWAY);
    let error: ApiError = serde_json::from_slice(&body).expect("error json");
    assert!(
        error.error.contains("daemon unexpectedly replied 500"),
        "{error:?}"
    );
}

#[tokio::test]
async fn serves_static_files() {
    let stub = Stub::start(sample_status()).await;

    let (code, body) = request("/style.css", stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let css = String::from_utf8(body).expect("utf8 stylesheet");
    assert!(css.contains("--lockbox-text:"), "{css}");
    assert!(css.contains("color-scheme: light dark"), "{css}");
}

#[tokio::test]
async fn serves_static_files_with_their_content_type() {
    let stub = Stub::start(sample_status()).await;

    let response = router(AppState::new(DaemonClient::new(stub.socket())))
        .oneshot(
            Request::builder()
                .uri("/style.css")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("web client must not fail");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/css; charset=utf-8"
    );
}

#[tokio::test]
async fn refuses_paths_not_in_the_static_folder() {
    let stub = Stub::start(sample_status()).await;

    for path in ["/missing.css", "/../Cargo.toml", "/static/style.css"] {
        let (code, _) = request(path, stub.socket()).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{path}");
    }
}
