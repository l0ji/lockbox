//! Tests of the bookmark and save-entry routes.

use axum::{Router, routing::get};
use lockbox_proto::api::{
    ApiError, Code, Page,
    node::SETTINGS_PATH,
    reader::{
        BOOKMARKS_PATH, SaveEntry, TopicsRequest, bookmark_path, bookmark_topics_path,
        save_entry_path,
    },
};
use lockbox_types::{bookmark::Bookmark, visibility::Visibility};

use crate::common::{Daemon, Site, json, request};
use crate::fixtures::{RSS, add, entries, folder, site};

async fn save(daemon: &Daemon, body: &str) -> (u16, String) {
    request(daemon.socket(), "POST", BOOKMARKS_PATH, Some(body)).await
}

async fn saved(daemon: &Daemon, body: &str) -> Bookmark {
    let (code, body) = save(daemon, body).await;
    assert!(code == 200 || code == 201, "{body}");
    json(&body)
}

async fn list(daemon: &Daemon, query: &str) -> Page<Bookmark> {
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{BOOKMARKS_PATH}{query}"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

#[tokio::test]
async fn saves_a_bookmark_with_its_topics_and_folder() {
    let daemon = Daemon::start().await;
    let folder = folder(&daemon, r#"{"name":"Reading"}"#).await;

    let bookmark = saved(
        &daemon,
        &serde_json::json!({
            "url": "https://www.Example.com/a/?utm_source=feed#top",
            "title": "A post",
            "note": "read later",
            "folder_id": folder.id,
            "topics": ["Rust", "databases"],
            "visibility": "public",
        })
        .to_string(),
    )
    .await;

    assert_eq!(
        bookmark.url,
        "https://www.Example.com/a/?utm_source=feed#top"
    );
    assert_eq!(bookmark.canonical_url, "https://example.com/a");
    assert_eq!(bookmark.title, "A post");
    assert_eq!(bookmark.note.as_deref(), Some("read later"));
    assert_eq!(bookmark.folder_id, Some(folder.id));
    assert_eq!(bookmark.visibility, Visibility::Public);
    assert_eq!(
        bookmark
            .topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["databases", "Rust"]
    );

    let page = list(&daemon, "").await;
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, bookmark.id);

    let (code, body) = request(daemon.socket(), "GET", &bookmark_path(bookmark.id), None).await;
    assert_eq!(code, 200, "{body}");
    let read: Bookmark = json(&body);
    assert_eq!(read, bookmark);
}

#[tokio::test]
async fn saves_the_same_link_once() {
    let daemon = Daemon::start().await;
    let first = saved(
        &daemon,
        &serde_json::json!({
            "url": "https://example.com/a",
            "title": "First",
            "visibility": "public",
        })
        .to_string(),
    )
    .await;

    let (code, body) = save(
        &daemon,
        &serde_json::json!({
            "url": "https://example.com/a?utm_source=feed",
            "title": "Second",
            "topics": ["Other"],
            "visibility": "private",
        })
        .to_string(),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let same: Bookmark = json(&body);
    assert_eq!(same.id, first.id);
    assert_eq!(same.title, "First");
    assert_eq!(same.visibility, Visibility::Public);
    assert!(same.topics.is_empty(), "a second save does not retag");
    assert_eq!(list(&daemon, "").await.items.len(), 1);
}

#[tokio::test]
async fn hides_archived_links_by_default() {
    let daemon = Daemon::start().await;
    let live = saved(
        &daemon,
        r#"{"url":"https://a.example","title":"Live","visibility":"public"}"#,
    )
    .await;
    let archived = saved(
        &daemon,
        r#"{"url":"https://b.example","title":"Archived","visibility":"public"}"#,
    )
    .await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &bookmark_path(archived.id),
        Some(r#"{"archived":true}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");

    let page = list(&daemon, "").await;
    assert_eq!(
        page.items
            .iter()
            .map(|bookmark| bookmark.id)
            .collect::<Vec<_>>(),
        [live.id]
    );
    let page = list(&daemon, "?archived=true").await;
    assert_eq!(
        page.items
            .iter()
            .map(|bookmark| bookmark.id)
            .collect::<Vec<_>>(),
        [archived.id]
    );
}

#[tokio::test]
async fn changes_a_bookmark_and_its_visibility() {
    let daemon = Daemon::start().await;
    let folder = folder(&daemon, r#"{"name":"Reading"}"#).await;
    let bookmark = saved(
        &daemon,
        r#"{"url":"https://a.example","title":"First","visibility":"public"}"#,
    )
    .await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &bookmark_path(bookmark.id),
        Some(
            &serde_json::json!({
                "title": "Better",
                "note": "a note",
                "url": "https://a.example/one",
                "folder_id": folder.id,
                "position": 0,
                "visibility": "private",
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let changed: Bookmark = json(&body);
    assert_eq!(changed.title, "Better");
    assert_eq!(changed.note.as_deref(), Some("a note"));
    assert_eq!(changed.url, "https://a.example/one");
    assert_eq!(changed.canonical_url, "https://a.example/one");
    assert_eq!(changed.folder_id, Some(folder.id));
    assert_eq!(changed.visibility, Visibility::Private);
}

#[tokio::test]
async fn replaces_the_topics_of_a_bookmark() {
    let daemon = Daemon::start().await;
    let bookmark = saved(
        &daemon,
        r#"{"url":"https://a.example","title":"First","topics":["One"],"visibility":"public"}"#,
    )
    .await;

    let request_body = TopicsRequest {
        topics: vec!["Rust".to_owned(), "Async".to_owned()],
    };
    let (code, body) = request(
        daemon.socket(),
        "PUT",
        &bookmark_topics_path(bookmark.id),
        Some(&serde_json::to_string(&request_body).expect("request")),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let changed: Bookmark = json(&body);
    assert_eq!(
        changed
            .topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        ["Async", "Rust"]
    );

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{BOOKMARKS_PATH}?topic=Rust"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Bookmark> = json(&body);
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn saves_an_entry_with_the_form_settings() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    add(&daemon, &site.url("/feed.xml")).await;
    let entry = entries(&daemon, "?query=First").await.items.remove(0);
    let folder = folder(&daemon, r#"{"name":"Reading"}"#).await;

    let request_body = SaveEntry {
        folder_id: Some(folder.id),
        topics: vec!["Rust".to_owned()],
        note: Some("from the feed".to_owned()),
        visibility: Some(Visibility::Private),
    };
    let (code, body) = request(
        daemon.socket(),
        "POST",
        &save_entry_path(entry.id),
        Some(&serde_json::to_string(&request_body).expect("request")),
    )
    .await;
    assert_eq!(code, 201, "{body}");
    let bookmark: Bookmark = json(&body);
    assert_eq!(bookmark.entry_id, Some(entry.id));
    assert_eq!(bookmark.title, "First post");
    assert_eq!(bookmark.url, "https://example.com/first");
    assert_eq!(bookmark.folder_id, Some(folder.id));
    assert_eq!(bookmark.visibility, Visibility::Private);
    assert_eq!(bookmark.note.as_deref(), Some("from the feed"));

    // Saving again returns the existing bookmark
    let (code, body) = request(
        daemon.socket(),
        "POST",
        &save_entry_path(entry.id),
        Some(&serde_json::to_string(&request_body).expect("request")),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let same: Bookmark = json(&body);
    assert_eq!(same.id, bookmark.id);

    // Saving preserves the entry's read state
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &lockbox_proto::api::reader::entry_path(entry.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let read: lockbox_types::entry::Entry = json(&body);
    assert_eq!(read.summary.read_at, None);
}

#[tokio::test]
async fn refuses_an_entry_without_a_link() {
    let daemon = Daemon::start().await;
    let site = Site::start(Router::new().route(
        "/feed.xml",
        get(|| async {
            r#"<?xml version="1.0"?><rss version="2.0"><channel><title>No links</title>
               <item><title>Only a title</title><guid>one</guid></item></channel></rss>"#
        }),
    ))
    .await;
    add(&daemon, &site.url("/feed.xml")).await;
    let entry = entries(&daemon, "").await.items.remove(0);

    let (code, body) = request(
        daemon.socket(),
        "POST",
        &save_entry_path(entry.id),
        Some("{}"),
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn refuses_a_link_which_is_not_http() {
    let daemon = Daemon::start().await;
    let (code, body) = save(
        &daemon,
        r#"{"url":"mailto:jane@example.com","title":"Mail","visibility":"public"}"#,
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn refuses_a_duplicate_address_on_edit() {
    let daemon = Daemon::start().await;
    let one = saved(
        &daemon,
        r#"{"url":"https://one.example","title":"One","visibility":"public"}"#,
    )
    .await;
    saved(
        &daemon,
        r#"{"url":"https://two.example","title":"Two","visibility":"public"}"#,
    )
    .await;

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &bookmark_path(one.id),
        Some(r#"{"url":"https://two.example"}"#),
    )
    .await;
    assert_eq!(code, 409, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Conflict);
}

#[tokio::test]
async fn refuses_a_cursor_made_for_another_order() {
    let daemon = Daemon::start().await;
    let folder = folder(&daemon, r#"{"name":"Reading"}"#).await;
    for (name, url) in [("a", "https://a.example"), ("b", "https://b.example")] {
        saved(
            &daemon,
            &serde_json::json!({
                "url": url,
                "title": name,
                "folder_id": folder.id,
                "visibility": "public",
            })
            .to_string(),
        )
        .await;
    }

    let ordered = list(&daemon, &format!("?folder={}&limit=1", folder.id)).await;
    let cursor = ordered.next.expect("next page");
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{BOOKMARKS_PATH}?cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);

    let recent = list(&daemon, "?limit=1").await;
    let cursor = recent.next.expect("next page");
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{BOOKMARKS_PATH}?folder={}&cursor={cursor}", folder.id),
        None,
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn follows_the_publishing_default_for_new_links() {
    let daemon = Daemon::start().await;
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        SETTINGS_PATH,
        Some(r#"{"publish_bookmarks":false}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");

    let private = saved(
        &daemon,
        r#"{"url":"https://private.example","title":"Private"}"#,
    )
    .await;
    assert_eq!(private.visibility, Visibility::Private);

    let public = saved(
        &daemon,
        r#"{"url":"https://public.example","title":"Public","visibility":"public"}"#,
    )
    .await;
    assert_eq!(public.visibility, Visibility::Public);
}

#[tokio::test]
async fn reports_a_bookmark_which_does_not_exist() {
    let daemon = Daemon::start().await;
    for (method, path, body) in [
        ("GET", bookmark_path(7), None),
        ("PATCH", bookmark_path(7), Some("{}")),
        ("DELETE", bookmark_path(7), None),
        ("PUT", bookmark_topics_path(7), Some(r#"{"topics":[]}"#)),
        ("POST", save_entry_path(7), Some("{}")),
    ] {
        let (code, body) = request(daemon.socket(), method, &path, body).await;
        assert_eq!(code, 404, "{method} {path}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound, "{method} {path}");
    }
}

#[tokio::test]
async fn deletes_a_bookmark() {
    let daemon = Daemon::start().await;
    let bookmark = saved(
        &daemon,
        r#"{"url":"https://a.example","title":"First","topics":["One"],"visibility":"public"}"#,
    )
    .await;

    let (code, body) = request(daemon.socket(), "DELETE", &bookmark_path(bookmark.id), None).await;
    assert_eq!(code, 204, "{body}");
    let (code, _) = request(daemon.socket(), "GET", &bookmark_path(bookmark.id), None).await;
    assert_eq!(code, 404);

    let (code, body) = request(daemon.socket(), "GET", BOOKMARKS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Bookmark> = json(&body);
    assert!(page.items.is_empty());
}
