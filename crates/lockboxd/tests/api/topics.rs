//! Tests of the topic routes.

use lockbox_proto::api::{
    ApiError, Code, Page,
    reader::{BOOKMARKS_PATH, TOPICS_PATH, topic_path},
};
use lockbox_types::topic::Topic;

use crate::common::{Daemon, json, request};

async fn bookmark(daemon: &Daemon, topics: &[&str]) {
    let body = serde_json::json!({
        "url": "https://example.com/a",
        "title": "A",
        "topics": topics,
        "visibility": "public",
    })
    .to_string();
    let (code, body) = request(daemon.socket(), "POST", BOOKMARKS_PATH, Some(&body)).await;
    assert!(code == 200 || code == 201, "{body}");
}

#[tokio::test]
async fn lists_and_pages_topics() {
    let daemon = Daemon::start().await;
    bookmark(&daemon, &["Rust", "Async", "Databases"]).await;

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{TOPICS_PATH}?limit=2"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Topic> = json(&body);
    assert_eq!(
        page.items
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Async", "Databases"]
    );
    let cursor = page.next.expect("next page");

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{TOPICS_PATH}?limit=2&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let rest: Page<Topic> = json(&body);
    assert_eq!(
        rest.items
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Rust"]
    );
    assert_eq!(rest.next, None);
}

#[tokio::test]
async fn renames_and_deletes_a_topic() {
    let daemon = Daemon::start().await;
    bookmark(&daemon, &["Rust"]).await;
    let (code, body) = request(daemon.socket(), "GET", TOPICS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Topic> = json(&body);
    let topic = page.items[0].clone();

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &topic_path(topic.id),
        Some(r#"{"name":"Systems"}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let renamed: Topic = json(&body);
    assert_eq!(renamed.name, "Systems");

    let (code, body) = request(daemon.socket(), "DELETE", &topic_path(topic.id), None).await;
    assert_eq!(code, 204, "{body}");
    let (code, _) = request(daemon.socket(), "GET", &topic_path(topic.id), None).await;
    assert_eq!(code, 404);
}

#[tokio::test]
async fn refuses_a_taken_topic_name() {
    let daemon = Daemon::start().await;
    bookmark(&daemon, &["Rust", "Async"]).await;
    let (code, body) = request(daemon.socket(), "GET", TOPICS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Topic> = json(&body);
    let rust = page
        .items
        .iter()
        .find(|topic| topic.name == "Rust")
        .expect("the rust topic");

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &topic_path(rust.id),
        Some(r#"{"name":"async"}"#),
    )
    .await;
    assert_eq!(code, 409, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Conflict);
}

#[tokio::test]
async fn reports_a_topic_which_does_not_exist() {
    let daemon = Daemon::start().await;
    for (method, body) in [
        ("GET", None),
        ("PATCH", Some(r#"{"name":"Rust"}"#)),
        ("DELETE", None),
    ] {
        let (code, body) = request(daemon.socket(), method, &topic_path(7), body).await;
        assert_eq!(code, 404, "{method}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound);
    }
}
