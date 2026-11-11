//! Tests of [`DaemonClient`] against a stub daemon's UNIX socket.

mod common;

use axum::{Json, Router, http::StatusCode, routing::get};
use common::{Stub, sample_status};
use lockbox_proto::api::{
    ApiError, Code,
    node::{STATUS_PATH, Status},
};
use lockbox_web::DaemonClient;

#[tokio::test]
async fn reads_a_reply_from_the_daemon() {
    let status = sample_status();
    let stub = Stub::start(status.clone()).await;
    let client = DaemonClient::new(stub.socket());

    let reply = client.get(STATUS_PATH).await.expect("daemon must answer");
    assert_eq!(reply.status, 200);
    assert!(reply.is_success());

    let read = client
        .get_json::<Status>(STATUS_PATH)
        .await
        .expect("status must parse");
    assert_eq!(read, status);
}

#[tokio::test]
async fn reports_an_unreachable_daemon() {
    let missing = std::env::temp_dir().join("lockbox-web-test-missing.sock");
    let _ = std::fs::remove_file(&missing);

    let error = DaemonClient::new(missing)
        .get_json::<Status>(STATUS_PATH)
        .await
        .expect_err("a missing socket must fail");
    assert!(
        error.to_string().contains("the daemon is offline"),
        "{error}"
    );
}

#[tokio::test]
async fn reports_a_bad_reply_status() {
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

    let error = DaemonClient::new(stub.socket())
        .get_json::<Status>(STATUS_PATH)
        .await
        .expect_err("a 500 must fail");
    assert!(
        error
            .to_string()
            .contains("daemon unexpectedly replied 500"),
        "{error}"
    );
}

#[tokio::test]
async fn reports_an_unparseable_reply() {
    let app = Router::new().route(STATUS_PATH, get(|| async { "not json" }));
    let stub = Stub::serve(app).await;

    let error = DaemonClient::new(stub.socket())
        .get_json::<Status>(STATUS_PATH)
        .await
        .expect_err("invalid json must fail");
    assert!(
        error.to_string().contains("cannot parse daemon reply"),
        "{error}"
    );
}
