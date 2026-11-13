//! Tests of the topic page and its forms.

use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::{get, patch},
};
use lockbox_proto::api::{ApiError, Code, Page, reader::TOPICS_PATH};
use lockbox_types::topic::Topic;

use crate::common::{Record, Stub, request, sample_topic};
use crate::stub;

#[tokio::test]
async fn serves_the_topics() {
    let stub = Stub::serve(stub(Record::default(), StatusCode::CREATED)).await;

    let (code, _, html) = request(Method::GET, "/topics", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("<title>lockbox: topics</title>"), "{html}");
    assert!(html.contains("Rust"), "{html}");
    assert!(html.contains(r#"action="/topics/1/rename""#), "{html}");
    assert!(html.contains(r#"action="/topics/1/delete""#), "{html}");
    assert!(html.contains(r#"href="/bookmarks?topic=Rust""#), "{html}");
}

#[tokio::test]
async fn renames_a_topic() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/topics/1/rename",
        Some("name=Systems"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/topics");
    let body: lockbox_types::topic::TopicChange =
        serde_json::from_str(&record.bodies()[0]).expect("topic json");
    assert_eq!(body.name.as_deref(), Some("Systems"));
}

#[tokio::test]
async fn deletes_a_topic() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) =
        request(Method::POST, "/topics/1/delete", Some(""), stub.socket()).await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/topics");
    assert_eq!(record.bodies().len(), 1);
}

#[tokio::test]
async fn pages_the_topics() {
    let app = Router::new().route(
        TOPICS_PATH,
        get(|| async {
            Json(Page {
                items: vec![sample_topic()],
                next: Some("abc".to_owned()),
            })
        }),
    );
    let stub = Stub::serve(app).await;

    let (_, _, html) = request(Method::GET, "/topics", None, stub.socket()).await;
    assert!(html.contains("more topics"), "{html}");
    assert!(html.contains("/topics?cursor=abc"), "{html}");
}

#[tokio::test]
async fn reports_a_topic_which_cannot_be_renamed() {
    let app = Router::new()
        .route(
            TOPICS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_topic()],
                    next: None,
                })
            }),
        )
        .route(
            &format!("{}/{{id}}", lockbox_proto::api::reader::TOPICS_PATH),
            patch(|| async {
                (
                    StatusCode::CONFLICT,
                    Json(ApiError {
                        code: Code::Conflict,
                        error: "UNIQUE constraint failed: topic.name".to_owned(),
                    }),
                )
            }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(
        Method::POST,
        "/topics/1/rename",
        Some("name=taken"),
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "the page is shown again");
    assert!(html.contains("UNIQUE constraint failed"), "{html}");
}

#[tokio::test]
async fn serves_an_empty_topic_page() {
    let app = Router::new().route(
        TOPICS_PATH,
        get(|| async {
            Json(Page {
                items: Vec::<Topic>::new(),
                next: None,
            })
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/topics", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("no topics yet"), "{html}");
}
