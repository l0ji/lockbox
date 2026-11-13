//! Tests of the saved link list, the archive page and the new bookmark page.

use axum::{
    Json, Router,
    http::{Method, StatusCode, header},
    routing::{get, post},
};
use lockbox_proto::api::{
    ApiError, Code, Page,
    node::SETTINGS_PATH,
    reader::{BOOKMARKS_PATH, FOLDERS_PATH, TOPICS_PATH},
};
use lockbox_types::{bookmark::Bookmark, folder::Folder};

use crate::common::{Record, Stub, request, sample_bookmark, sample_topic};
use crate::{list_stub, stub};

#[tokio::test]
async fn serves_the_saved_links() {
    let stub = Stub::serve(stub(Record::default(), StatusCode::CREATED)).await;

    let (code, _, html) = request(Method::GET, "/bookmarks", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("<title>lockbox: bookmarks</title>"), "{html}");
    assert!(
        html.contains(r#"<a class="tab on" href="/bookmarks">bookmarks</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a class="button" href="/bookmark/new">new bookmark</a>"#),
        "{html}"
    );
    assert!(html.contains("First post"), "{html}");
    assert!(html.contains("example.com"), "{html}");
    assert!(
        html.contains(r#"<a class="chip" href="/bookmarks?topic=Rust">Rust</a>"#),
        "{html}"
    );
    assert!(html.contains("🌐"), "{html}");
    assert!(!html.contains("PUBLIC"), "{html}");
    assert!(!html.contains("PRIVATE"), "{html}");
    assert!(
        html.contains(r#"<h2 class="rail-section">folders</h2>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<h2 class="rail-section">topics</h2>"#),
        "{html}"
    );
    assert!(
        html.contains("├─ Rust") || html.contains("└─ Rust"),
        "{html}"
    );
    assert!(!html.contains("manage folders"), "{html}");
    assert!(!html.contains("all topics"), "{html}");
}

#[tokio::test]
async fn escapes_topic_links_on_each_page() {
    let topic = lockbox_types::topic::Topic {
        name: "Rust / C++ & λ".to_owned(),
        ..sample_topic()
    };
    let bookmark = Bookmark {
        topics: vec![topic.clone()],
        ..sample_bookmark()
    };
    let app = Router::new()
        .route(
            BOOKMARKS_PATH,
            get(move || {
                let bookmark = bookmark.clone();
                async move {
                    Json(Page {
                        items: vec![bookmark],
                        next: None,
                    })
                }
            }),
        )
        .route(FOLDERS_PATH, get(crate::folders))
        .route(
            TOPICS_PATH,
            get(move || {
                let topic = topic.clone();
                async move {
                    Json(Page {
                        items: vec![topic],
                        next: None,
                    })
                }
            }),
        );
    let stub = Stub::serve(app).await;
    let link = r#"href="/bookmarks?topic=Rust%20%2F%20C%2B%2B%20%26%20%CE%BB""#;

    for (path, count) in [("/bookmarks", 2), ("/topics", 1)] {
        let (code, _, html) = request(Method::GET, path, None, stub.socket()).await;
        assert_eq!(code, StatusCode::OK, "{path}: {html}");
        assert_eq!(html.matches(link).count(), count, "{path}: {html}");
    }
}

#[tokio::test]
async fn serves_the_archived_page() {
    let bookmark = Bookmark {
        archived_at: Some(1_700_000_200),
        ..sample_bookmark()
    };
    let stub = Stub::serve(list_stub(vec![bookmark], None)).await;

    let (code, _, html) = request(Method::GET, "/bookmarks/archived", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(html.contains("<title>lockbox: archived</title>"), "{html}");
    assert!(html.contains("First post"), "{html}");
    assert!(html.contains("unarchive"), "{html}");
}

#[tokio::test]
async fn serves_the_new_bookmark_page() {
    let stub = Stub::serve(stub(Record::default(), StatusCode::CREATED)).await;

    let (code, _, html) = request(Method::GET, "/bookmark/new", None, stub.socket()).await;
    assert_eq!(code, StatusCode::OK);
    assert!(
        html.contains("<title>lockbox: new bookmark</title>"),
        "{html}"
    );
    assert!(
        html.contains(r#"<form class="stack" method="post" action="/bookmarks">"#),
        "{html}"
    );
    assert!(html.contains("name=\"url\""), "{html}");
    assert!(html.contains("name=\"topics\""), "{html}");
    assert!(html.contains("name=\"visibility\""), "{html}");
    assert!(!html.contains("nothing saved yet"), "{html}");
}

#[tokio::test]
async fn creates_a_bookmark() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/bookmarks",
        Some(
            "url=https%3A%2F%2Fexample.com%2Ffirst&title=First&note=a+note&folder_id=1&topics=rust%2C+async&visibility=private",
        ),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/bookmark/1");
    let body: lockbox_proto::api::reader::BookmarkRequest =
        serde_json::from_str(&record.bodies()[0]).expect("bookmark json");
    assert_eq!(body.url, "https://example.com/first");
    assert_eq!(body.title.as_deref(), Some("First"));
    assert_eq!(body.note.as_deref(), Some("a note"));
    assert_eq!(body.folder_id, Some(1));
    assert_eq!(body.topics, ["rust", "async"]);
    assert_eq!(
        body.visibility,
        Some(lockbox_types::visibility::Visibility::Private)
    );
}

#[tokio::test]
async fn reports_a_bookmark_which_cannot_be_saved() {
    let app = Router::new()
        .route(
            BOOKMARKS_PATH,
            post(|| async {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: Code::Invalid,
                        error: "https://example.com: not an HTTP address".to_owned(),
                    }),
                )
            }),
        )
        .route(FOLDERS_PATH, get(|| async { Json(Vec::<Folder>::new()) }))
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        );
    let stub = Stub::serve(app).await;

    let (code, _, html) = request(
        Method::POST,
        "/bookmarks",
        Some("url=example.com&title=&note=&folder_id=&topics=&visibility=public"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::OK, "the form is shown again");
    assert!(html.contains("not an HTTP address"), "{html}");
}

#[tokio::test]
async fn archives_a_bookmark() {
    let record = Record::default();
    let stub = Stub::serve(stub(record.clone(), StatusCode::CREATED)).await;

    let (code, headers, _) = request(
        Method::POST,
        "/bookmark/1/archive",
        Some("on=true&back=%2Fbookmarks"),
        stub.socket(),
    )
    .await;

    assert_eq!(code, StatusCode::SEE_OTHER);
    assert_eq!(headers[header::LOCATION], "/bookmarks");
    assert!(
        record.bodies()[0].contains(r#""archived":true"#),
        "{:?}",
        record.bodies()
    );
}

#[tokio::test]
async fn links_to_the_next_page_and_the_archive() {
    let stub = Stub::serve(list_stub(vec![sample_bookmark()], Some("abc".to_owned()))).await;

    let (_, _, html) = request(Method::GET, "/bookmarks", None, stub.socket()).await;
    assert!(html.contains("older links"), "{html}");
    assert!(html.contains(r#"href="/bookmarks/archived""#), "{html}");
}

#[tokio::test]
async fn keeps_a_page_of_topics_and_folders_in_the_rail() {
    let app = Router::new()
        .route(
            BOOKMARKS_PATH,
            get(|| async {
                Json(Page {
                    items: Vec::<Bookmark>::new(),
                    next: None,
                })
            }),
        )
        .route(FOLDERS_PATH, get(|| async { Json(Vec::<Folder>::new()) }))
        .route(
            lockbox_proto::api::reader::TOPICS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_topic()],
                    next: None,
                })
            }),
        )
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        );
    let stub = Stub::serve(app).await;

    let (_, _, html) = request(Method::GET, "/bookmarks", None, stub.socket()).await;
    assert!(html.contains("nothing saved yet"), "{html}");
    assert!(html.contains("Rust"), "{html}");
}
