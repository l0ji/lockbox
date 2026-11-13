//! Tests of the feed page and the forms which it provides.

use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::{get, post},
};
use lockbox_proto::api::{ApiError, Code, Page, reader};
use lockbox_types::feed::{Feed, FeedChange};

use crate::common::{Record, Stub, request, sample_feed};
use crate::lists;
use crate::recorded;

#[tokio::test]
async fn serves_the_feed_list() {
    let stub = Stub::serve(lists(sample_feed(), Vec::new())).await;

    let (code, _, html) = request(Method::GET, "/feeds", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("Example blog"), "{html}");
    assert!(html.contains("https://example.com/feed.xml"), "{html}");
    assert!(html.contains("<td>1h</td>"), "{html}");
    assert!(html.contains(r#"action="/feeds/1/refresh""#), "{html}");
    assert!(html.contains(r#"action="/feeds/1/delete""#), "{html}");
    assert!(html.contains(r#"action="/feeds/1/settings""#), "{html}");
    assert!(
        html.contains(r#"<input type="number" name="timeout_seconds" value="20""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="subtab on" href="/feeds">feeds</a>"#),
        "{html}"
    );
}

#[tokio::test]
async fn adds_a_feed() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/feeds",
        Some("url=https%3A%2F%2Fexample.com%2Ffeed.xml"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(
        headers[header::LOCATION].to_str().expect("location"),
        "/feeds"
    );
    let expected = concat!(
        r#"{"url":"https://example.com/feed.xml","title":null,"refresh_seconds":null,"#,
        r#""timeout_seconds":null,"user_agent":null}"#
    );
    assert_eq!(record.bodies(), [expected]);
}

#[tokio::test]
async fn reports_a_feed_which_cannot_be_added() {
    let app = Router::new()
        .route(
            reader::FEEDS_PATH,
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: Code::Invalid,
                        error: "https://example.com is neither a feed nor a page with one"
                            .to_owned(),
                    }),
                )
            }),
        )
        .route(
            reader::FEEDS_PATH,
            get(|| async {
                Json(Page {
                    items: Vec::<Feed>::new(),
                    next: None,
                })
            }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(
        Method::POST,
        "/feeds",
        Some("url=https%3A%2F%2Fexample.com"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::OK, "the form is shown again");
    assert!(
        html.contains("neither a feed nor a page with one"),
        "{html}"
    );
}

#[tokio::test]
async fn refreshes_and_removes_a_feed() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) =
        request(Method::POST, "/feeds/1/refresh", Some(""), stub.socket()).await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/feeds");

    let (code, headers, _) =
        request(Method::POST, "/feeds/1/delete", Some(""), stub.socket()).await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/feeds");
    assert_eq!(record.bodies().len(), 2);
}

#[tokio::test]
async fn changes_how_a_feed_is_fetched() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/feeds/1/settings",
        Some("timeout_seconds=90&user_agent=another+reader"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/feeds");
    let body: FeedChange = serde_json::from_str(&record.bodies()[0]).expect("feed change json");
    assert_eq!(body.timeout_seconds, Some(90));
    assert_eq!(body.user_agent, Some(Some("another reader".to_owned())));
}

#[tokio::test]
async fn returns_a_feed_to_the_default_user_agent() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, _, _) = request(
        Method::POST,
        "/feeds/1/settings",
        Some("timeout_seconds=20&user_agent="),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    let body: FeedChange = serde_json::from_str(&record.bodies()[0]).expect("feed change json");
    assert_eq!(body.user_agent, Some(None), "an empty field clears it");
}
