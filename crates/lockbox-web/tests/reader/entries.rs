//! Tests of the reading list and the single entry page.

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::get,
};
use lockbox_proto::api::{
    ApiError, Code, Page,
    node::SETTINGS_PATH,
    reader::{self, FOLDERS_PATH},
};
use lockbox_types::{
    entry::{Entry, EntrySummary},
    feed::Feed,
    folder::Folder,
};
use lockbox_web::TEXT_LIMIT;

use crate::common::{Record, Stub, request, sample_body, sample_entry, sample_feed};
use crate::lists;
use crate::recorded;

#[tokio::test]
async fn serves_the_reading_list() {
    let stub = Stub::serve(lists(sample_feed(), vec![sample_entry()])).await;

    let (code, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("First post"), "{html}");
    assert!(html.contains("Example blog"), "{html}");
    assert!(
        html.contains(r#"<a class="title" href="https://example.com/first""#),
        "{html}"
    );
    assert!(!html.contains(r#"href="/entry/1""#), "{html}");
    assert!(
        html.contains(r#"<form class="act read" method="post""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<form class="act star" method="post""#),
        "{html}"
    );
    assert!(
        html.contains(r#"action="/entry/1/read""#) && html.contains(r#"action="/entry/1/star""#),
        "{html}"
    );
    assert!(
        html.contains(r#"action="/entry/1/save""#)
            && html.contains(r#"id="save-1""#)
            && html.contains("name=\"visibility\"")
            && html.contains("name=\"topics\"")
            && html.contains("name=\"note\""),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab on" href="/">reader</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/bookmarks">bookmarks</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/node">node</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="subtab on" href="/">entries</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="subtab" href="/feeds">feeds</a>"#),
        "{html}"
    );
    assert!(html.contains("<h1 class=\"section\">reader</h1>"), "{html}");
    assert!(html.contains(r#"href="/?read=false""#), "{html}");
}

#[tokio::test]
async fn leaves_folders_and_topics_to_the_bookmarks_view() {
    let stub = Stub::serve(lists(sample_feed(), vec![sample_entry()])).await;

    let (_, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert!(!html.contains(">folders</h2>"), "{html}");
    assert!(!html.contains(">topics</h2>"), "{html}");
    assert!(!html.contains("manage folders"), "{html}");
    assert!(!html.contains("all topics"), "{html}");
}

#[tokio::test]
async fn offers_every_feed_on_both_views() {
    let stub = Stub::serve(lists(sample_feed(), vec![sample_entry()])).await;

    let (_, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert!(
        html.contains(r#"<li class="on"><a href="/">all feeds</a></li>"#),
        "{html}"
    );

    let (code, _, html) = request(Method::GET, "/?feed=1", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains(r#"<a href="/">all feeds</a>"#), "{html}");
    assert!(
        html.contains(r#"<li class="on"><a href="/?feed=1">Example blog</a></li>"#),
        "{html}"
    );
}

#[tokio::test]
async fn cuts_a_long_summary_in_the_list() {
    let long = "word ".repeat(100);
    let mut entry = sample_entry();
    entry.summary = Some(long.clone());
    let stub = Stub::serve(lists(sample_feed(), vec![entry])).await;

    let (code, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let cut = long.chars().take(TEXT_LIMIT).collect::<String>();
    assert!(html.contains(&cut), "{html}");
    assert!(html.contains("..."), "{html}");
    assert!(!html.contains(&long), "the rest is cut: {html}");
}

#[tokio::test]
async fn cuts_a_long_summary_on_the_entry_page() {
    let long = "word ".repeat(100);
    let mut body = sample_body();
    body.body = None;
    body.summary.summary = Some(long.clone());
    let app = lists(sample_feed(), vec![sample_entry()]).route(
        "/api/reader/entries/{id}",
        get(move || {
            let body = body.clone();
            async move { Json(body) }
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/entry/1", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    let cut = long.chars().take(TEXT_LIMIT).collect::<String>();
    assert!(html.contains(&cut), "{html}");
    assert!(!html.contains(&long), "the rest is cut: {html}");
}

#[tokio::test]
async fn serves_an_empty_reading_list() {
    let stub = Stub::serve(lists(sample_feed(), Vec::new())).await;

    let (code, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("nothing to read"), "{html}");
}

#[tokio::test]
async fn forwards_the_reading_list_filter() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded_paths = Arc::clone(&seen);
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
            get(move |uri: axum::http::Uri| {
                let seen = Arc::clone(&recorded_paths);
                async move {
                    seen.lock()
                        .expect("paths")
                        .push(uri.query().unwrap_or_default().to_owned());
                    Json(Page {
                        items: Vec::<EntrySummary>::new(),
                        next: None,
                    })
                }
            }),
        )
        .route(FOLDERS_PATH, get(|| async { Json(Vec::<Folder>::new()) }))
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, _) = request(
        Method::GET,
        "/?feed=3&read=false&query=two+words",
        None,
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        seen.lock().expect("paths").first().map(String::as_str),
        Some("feed=3&read=false&query=two%20words")
    );
}

#[tokio::test]
async fn serves_one_entry() {
    let app = lists(sample_feed(), vec![sample_entry()]).route(
        "/api/reader/entries/{id}",
        get(|| async { Json(sample_body()) }),
    );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/entry/1", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("First post"), "{html}");
    assert!(html.contains("Jane"), "{html}");
    assert!(html.contains("words"), "{html}");
    assert!(
        !html.contains("<p>words</p>"),
        "the body is escaped: {html}"
    );
    assert!(
        html.contains(r#"href="https://example.com/first""#),
        "{html}"
    );
    assert!(!html.contains(r#"action="/entry/1/read""#), "{html}");
    assert!(!html.contains(r#"action="/entry/1/star""#), "{html}");
}

#[tokio::test]
async fn marks_an_entry_read() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/entry/1/read",
        Some("on=true"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/", "the list is the default");
    assert_eq!(
        record.bodies(),
        [r#"{"read":true,"starred":null,"archived":null}"#]
    );
}

#[tokio::test]
async fn saves_an_entry_with_the_dialog_fields() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/entry/1/save",
        Some(
            "visibility=private&folder_id=&topics=rust%2C+async&note=read+later&back=%2F%3Ffeed%3D1",
        ),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/?feed=1");
    let body: lockbox_proto::api::reader::SaveEntry =
        serde_json::from_str(&record.bodies()[0]).expect("save json");
    assert_eq!(body.folder_id, None);
    assert_eq!(body.topics, ["rust", "async"]);
    assert_eq!(body.note.as_deref(), Some("read later"));
    assert_eq!(
        body.visibility,
        Some(lockbox_types::visibility::Visibility::Private)
    );
}

#[tokio::test]
async fn asks_for_the_page_the_button_was_pressed_on() {
    let record = Record::default();
    let stub = Stub::serve(recorded(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/entry/1/star",
        Some("on=true&back=%2F%3Ffeed%3D1%26read%3Dfalse"),
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/?feed=1&read=false");

    // Reject external redirect targets
    let (code, headers, _) = request(
        Method::POST,
        "/entry/1/star",
        Some("on=false&back=https%3A%2F%2Fexample.com%2F"),
        stub.socket(),
    )
    .await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/");
}

#[tokio::test]
async fn reports_a_daemon_which_is_down() {
    let missing = std::env::temp_dir().join("lockbox-web-test-missing.sock");
    let _ = std::fs::remove_file(&missing);

    let (code, _, html) = request(Method::GET, "/", None, missing).await;

    assert_eq!(code, StatusCode::BAD_GATEWAY);
    assert!(html.contains("the daemon is offline"), "{html}");
    assert!(
        !html.contains("lockbox-web-test-missing.sock"),
        "the socket path stays out of the page: {html}"
    );
}

#[tokio::test]
async fn reports_an_entry_which_does_not_exist() {
    let app = lists(sample_feed(), vec![sample_entry()]).route(
        "/api/reader/entries/{id}",
        get(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    code: Code::NotFound,
                    error: "no such entry".to_owned(),
                }),
            )
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/entry/9", None, stub.socket()).await;

    assert_eq!(code, StatusCode::NOT_FOUND);
    assert!(html.contains("no such entry"), "{html}");
}

#[tokio::test]
async fn cuts_a_long_body_on_the_entry_page() {
    let long = "word ".repeat(100);
    let body = Entry {
        body: Some(long.clone()),
        ..sample_body()
    };
    let app = lists(sample_feed(), vec![sample_entry()]).route(
        "/api/reader/entries/{id}",
        get(move || {
            let body = body.clone();
            async move { Json(body) }
        }),
    );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(Method::GET, "/entry/1", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("..."), "{html}");
    let cut = long.chars().take(TEXT_LIMIT).collect::<String>();
    assert!(html.contains(&cut), "{html}");
    assert!(!html.contains(&long), "the rest is left alone: {html}");
}
