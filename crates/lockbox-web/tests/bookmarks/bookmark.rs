//! Tests of the single bookmark page and its forms.

use crate::common::{Record, Stub, request, sample_folder};
use crate::stub;
use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::get,
};
use lockbox_proto::api::{ApiError, Code, node::SETTINGS_PATH, reader::FOLDERS_PATH};

#[tokio::test]
async fn serves_one_bookmark() {
    let stub = Stub::serve(stub(Record::default(), StatusCode::CREATED)).await;

    let (code, _, html) = request(Method::GET, "/bookmark/1", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        html.contains("<title>lockbox: First post</title>"),
        "{html}"
    );
    assert!(html.contains(r#"action="/bookmark/1""#), "{html}");
    assert!(html.contains(r#"action="/bookmark/1/topics""#), "{html}");
    assert!(html.contains(r#"action="/bookmark/1/delete""#), "{html}");
    assert!(html.contains("example.com/first"), "{html}");
    assert!(html.contains("Rust"), "{html}");
    assert!(
        html.contains(r#"saved from <a href="/entry/1">entry 1</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<option value="1" selected>Reading</option>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<option value="public" selected>public</option>"#),
        "{html}"
    );
}

#[tokio::test]
async fn changes_a_bookmark() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/bookmark/1",
        Some(
            "title=Better&url=https%3A%2F%2Fexample.com%2Fbetter&note=a+note&folder_id=1&position=2&visibility=private&archived=on",
        ),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/bookmark/1");
    let body: lockbox_types::bookmark::BookmarkChange =
        serde_json::from_str(&record.bodies()[0]).expect("change json");
    assert_eq!(body.title.as_deref(), Some("Better"));
    assert_eq!(body.url.as_deref(), Some("https://example.com/better"));
    assert_eq!(body.note, Some(Some("a note".to_owned())));
    assert_eq!(body.folder_id, Some(Some(1)));
    assert_eq!(body.position, Some(2));
    assert_eq!(body.archived, Some(true));
    assert_eq!(
        body.visibility,
        Some(lockbox_types::visibility::Visibility::Private)
    );
}

#[tokio::test]
async fn replaces_the_topics() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/bookmark/1/topics",
        Some("topics=rust%2C+async"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/bookmark/1");
    let body: lockbox_proto::api::reader::TopicsRequest =
        serde_json::from_str(&record.bodies()[0]).expect("topics json");
    assert_eq!(body.topics, ["rust", "async"]);
}

#[tokio::test]
async fn deletes_a_bookmark() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) =
        request(Method::POST, "/bookmark/1/delete", Some(""), stub.socket()).await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/bookmarks");
    assert_eq!(record.bodies().len(), 1);
}

#[tokio::test]
async fn reports_a_bookmark_which_does_not_exist() {
    let app = Router::new()
        .route(
            &format!("{}/{{id}}", lockbox_proto::api::reader::BOOKMARKS_PATH),
            get(|| async {
                (
                    StatusCode::NOT_FOUND,
                    Json(ApiError {
                        code: Code::NotFound,
                        error: "no such bookmark".to_owned(),
                    }),
                )
            }),
        )
        .route(FOLDERS_PATH, get(|| async { Json(vec![sample_folder()]) }))
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/bookmark/9", None, stub.socket()).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    assert!(html.contains("no such bookmark"), "{html}");
}
