//! Tests of the reader pages and the forms which they provide.

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Json, Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
    routing::{delete, get, patch, post},
};
use common::{Stub, sample_body, sample_entry, sample_feed};
use lockbox_proto::api::{
    ApiError, Code, Page,
    reader::{self, Entry, EntrySummary, Feed, FeedChange, Refreshed},
};
use lockbox_web::{AppState, DaemonClient, TEXT_LIMIT, router};
use tower::ServiceExt;

/// One request against a fresh router, returned as (status, headers, body).
async fn request(
    method: Method,
    path: &str,
    form: Option<&str>,
    socket: impl Into<std::path::PathBuf>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let mut request = Request::builder().method(method).uri(path);
    let body = match form {
        Some(form) => {
            request = request
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .header(header::HOST, "localhost");
            Body::from(form.to_owned())
        }
        None => Body::empty(),
    };
    let request = request.body(body).expect("build request");

    let response = router(AppState::new(DaemonClient::new(socket.into())))
        .oneshot(request)
        .await
        .expect("web client must not fail");
    let status = response.status();
    let headers = response.headers().clone();
    let body = axum::body::to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("read body");
    (
        status,
        headers,
        String::from_utf8(body.to_vec()).expect("utf8 body"),
    )
}

/// A stub daemon which answers both reader list routes.
fn lists(feed: Feed, entries: Vec<EntrySummary>) -> Router {
    Router::new()
        .route(
            reader::FEEDS_PATH,
            get(move || {
                let feed = feed.clone();
                async move {
                    Json(Page {
                        items: vec![feed],
                        next: None,
                    })
                }
            }),
        )
        .route(
            reader::ENTRIES_PATH,
            get(move || {
                let entries = entries.clone();
                async move {
                    Json(Page {
                        items: entries.clone(),
                        next: None,
                    })
                }
            }),
        )
}

/// The bodies which a stub daemon was sent, in order.
#[derive(Debug, Clone, Default)]
struct Record(Arc<Mutex<Vec<String>>>);

impl Record {
    fn bodies(&self) -> Vec<String> {
        self.0.lock().expect("recorded bodies").clone()
    }

    fn push(&self, body: String) {
        self.0.lock().expect("recorded bodies").push(body);
    }
}

/// A stub daemon which records what the forms post and answers with `reply`.
fn recorded(record: Record, reply: StatusCode) -> Router {
    let add = record.clone();
    let refresh = record.clone();
    let remove = record.clone();
    let settings = record.clone();
    let mark = record.clone();

    Router::new()
        .route(
            reader::FEEDS_PATH,
            post(move |body: String| {
                let record = add.clone();
                async move {
                    record.push(body);
                    (
                        reply,
                        Json(Refreshed {
                            feed: sample_feed(),
                            new_entries: 2,
                        }),
                    )
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}/refresh",
            post(move |body: String| {
                let record = refresh.clone();
                async move {
                    record.push(body);
                    Json(Refreshed {
                        feed: sample_feed(),
                        new_entries: 1,
                    })
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}",
            delete(move || {
                let record = remove.clone();
                async move {
                    record.push(String::new());
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}",
            patch(move |body: String| {
                let record = settings.clone();
                async move {
                    record.push(body);
                    Json(sample_feed())
                }
            }),
        )
        .route(
            "/api/reader/entries/{id}",
            patch(move |body: String| {
                let record = mark.clone();
                async move {
                    record.push(body);
                    Json(sample_body())
                }
            }),
        )
        .route(
            reader::ENTRIES_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_entry()],
                    next: None,
                })
            }),
        )
        .route(
            reader::FEEDS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_feed()],
                    next: None,
                })
            }),
        )
}

#[tokio::test]
async fn serves_the_reading_list() {
    let stub = Stub::serve(lists(sample_feed(), vec![sample_entry()])).await;

    let (code, _, html) = request(Method::GET, "/", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("First post"), "{html}");
    assert!(html.contains("Example blog"), "{html}");
    // The title leads to the article itself, not to the entry's own page
    assert!(
        html.contains(r#"<a class="title" href="https://example.com/first""#),
        "{html}"
    );
    assert!(!html.contains(r#"href="/entry/1""#), "{html}");
    // The row carries the entry's two text buttons
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
    // The navbar marks the section and leads to the others
    assert!(
        html.contains(r#"<a class="tab on" href="/">reader</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/feeds">feeds</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="tab" href="/node">node</a>"#),
        "{html}"
    );
    assert!(html.contains("<h1 class=\"section\">reader</h1>"), "{html}");
    assert!(html.contains(r#"href="/?read=false""#), "{html}");
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
    // The page is available without claiming the actions of the list
    assert!(!html.contains(r#"action="/entry/1/read""#), "{html}");
    assert!(!html.contains(r#"action="/entry/1/star""#), "{html}");
}

#[tokio::test]
async fn serves_the_feed_list() {
    let stub = Stub::serve(lists(sample_feed(), vec![sample_entry()])).await;

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

    // A page elsewhere is never a place to answer to
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
