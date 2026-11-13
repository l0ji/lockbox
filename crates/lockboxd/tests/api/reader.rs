//! Tests of the feed and entry routes.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{Router, routing::get};
use lockbox_proto::api::{
    ApiError, Code, Page,
    reader::{self, DEFAULT_USER_AGENT, ENTRIES_PATH, FEEDS_PATH},
};
use lockbox_types::{
    entry::Entry,
    feed::{DEFAULT_TIMEOUT_SECONDS, Feed, Kind, Refreshed},
};

use crate::common::{Daemon, Site, json, request};
use crate::fixtures::{RSS, RSS_UPDATED, add, entries, page_site, site, titles};

#[tokio::test]
async fn adds_a_feed_and_the_entries_it_brought() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;

    let refreshed = add(&daemon, &site.url("/feed.xml")).await;
    assert_eq!(refreshed.new_entries, 2);
    assert_eq!(refreshed.feed.url, site.url("/feed.xml"));
    assert_eq!(refreshed.feed.title.as_deref(), Some("Example blog"));
    assert_eq!(
        refreshed.feed.site_url.as_deref(),
        Some("https://example.com/")
    );
    assert_eq!(refreshed.feed.kind, Kind::Rss);
    assert_eq!(refreshed.feed.refresh_seconds, 3600);
    assert!(refreshed.feed.fetched_at.is_some());

    assert_eq!(titles(&daemon, "").await, ["Second post", "First post"]);
}

#[tokio::test]
async fn adds_the_feed_which_a_page_advertises() {
    let daemon = Daemon::start().await;
    let site = page_site().await;

    let refreshed = add(&daemon, &site.url("/")).await;
    assert_eq!(refreshed.feed.url, site.url("/feed.xml"));
    assert_eq!(refreshed.new_entries, 2);
}

#[tokio::test]
async fn refuses_a_page_which_advertises_no_feed() {
    let daemon = Daemon::start().await;
    let site =
        Site::start(Router::new().route("/", get(|| async { "<html><body>words</body></html>" })))
            .await;

    let request_body = serde_json::json!({ "url": site.url("/") }).to_string();
    let (code, body) = request(daemon.socket(), "POST", FEEDS_PATH, Some(&request_body)).await;

    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
    assert!(error.error.contains("neither a feed"), "{error:?}");
    assert!(entries(&daemon, "").await.items.is_empty());
}

#[tokio::test]
async fn reports_a_feed_which_cannot_be_fetched() {
    let daemon = Daemon::start().await;
    let request_body = serde_json::json!({ "url": "http://127.0.0.1:1/feed.xml" }).to_string();
    let (code, body) = request(daemon.socket(), "POST", FEEDS_PATH, Some(&request_body)).await;

    assert_eq!(code, 502, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Internal);
    assert!(error.error.contains("cannot fetch"), "{error:?}");
}

#[tokio::test]
async fn records_a_failed_refresh_on_the_feed() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    let refreshed = add(&daemon, &site.url("/feed.xml")).await;

    // The next refresh fails after the site stops
    drop(site);
    let (code, body) = request(
        daemon.socket(),
        "POST",
        &reader::refresh_path(refreshed.feed.id),
        None,
    )
    .await;
    assert_eq!(code, 502, "{body}");

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &reader::feed_path(refreshed.feed.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let feed: Feed = json(&body);
    assert!(
        feed.last_error
            .as_deref()
            .is_some_and(|error| error.contains("cannot fetch")),
        "{:?}",
        feed.last_error
    );
    assert_eq!(titles(&daemon, "").await.len(), 2, "the entries stay");
}

#[tokio::test]
async fn refreshes_a_feed_again() {
    let daemon = Daemon::start().await;
    let served = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&served);
    let site = Site::start(Router::new().route(
        "/feed.xml",
        get(move || {
            let counter = Arc::clone(&counter);
            async move {
                match counter.fetch_add(1, Ordering::SeqCst) {
                    0 => RSS,
                    _ => RSS_UPDATED,
                }
            }
        }),
    ))
    .await;

    let refreshed = add(&daemon, &site.url("/feed.xml")).await;
    assert_eq!(refreshed.new_entries, 2);

    let (code, body) = request(
        daemon.socket(),
        "POST",
        &reader::refresh_path(refreshed.feed.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let refreshed: Refreshed = json(&body);
    assert_eq!(refreshed.new_entries, 1);
    assert_eq!(refreshed.feed.last_error, None);

    assert_eq!(
        titles(&daemon, "").await,
        ["Third post", "Second post", "First post"]
    );
}

#[tokio::test]
async fn filters_and_pages_the_reading_list() {
    let daemon = Daemon::start().await;
    let site = site(RSS_UPDATED).await;
    let refreshed = add(&daemon, &site.url("/feed.xml")).await;

    let page = entries(&daemon, "?limit=2").await;
    assert_eq!(
        page.items
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        ["Third post", "Second post"]
    );
    let cursor = page.next.expect("a page which is not the last");

    let rest = entries(&daemon, &format!("?limit=2&cursor={cursor}")).await;
    assert_eq!(
        rest.items
            .iter()
            .map(|entry| entry.title.as_str())
            .collect::<Vec<_>>(),
        ["First post"]
    );
    assert_eq!(rest.next, None);

    assert_eq!(
        entries(&daemon, &format!("?feed={}", refreshed.feed.id))
            .await
            .items
            .len(),
        3
    );
    assert_eq!(entries(&daemon, "?feed=999").await.items.len(), 0);

    assert_eq!(titles(&daemon, "?query=Third").await, ["Third post"]);
    assert!(entries(&daemon, "?query=nothing").await.items.is_empty());
}

#[tokio::test]
async fn reads_marks_and_deletes_an_entry() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    add(&daemon, &site.url("/feed.xml")).await;
    let mut page = entries(&daemon, "?query=First").await;
    let first = page.items.remove(0);

    let (code, body) = request(daemon.socket(), "GET", &reader::entry_path(first.id), None).await;
    assert_eq!(code, 200, "{body}");
    let entry: Entry = json(&body);
    assert_eq!(entry.summary.title, "First post");
    assert_eq!(entry.body.as_deref(), Some("<p>words</p>"));
    assert_eq!(entry.summary.read_at, None);
    assert!(!entry.truncated);

    // Read entries leave the unread list
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &reader::entry_path(first.id),
        Some(r#"{"read":true}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let entry: Entry = json(&body);
    assert!(entry.summary.read_at.is_some());
    assert_eq!(titles(&daemon, "?read=false").await, ["Second post"]);

    // Starred entries appear in the starred list
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &reader::entry_path(first.id),
        Some(r#"{"starred":true}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(titles(&daemon, "?starred=true").await, ["First post"]);

    let (code, body) = request(
        daemon.socket(),
        "DELETE",
        &reader::entry_path(first.id),
        None,
    )
    .await;
    assert_eq!(code, 204, "{body}");

    let (code, _) = request(daemon.socket(), "GET", &reader::entry_path(first.id), None).await;
    assert_eq!(code, 404);
    assert_eq!(titles(&daemon, "").await, ["Second post"]);
}

#[tokio::test]
async fn changes_and_removes_a_feed() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    let refreshed = add(&daemon, &site.url("/feed.xml")).await;
    let path = reader::feed_path(refreshed.feed.id);

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &path,
        Some(r#"{"title":"Renamed","refresh_seconds":600}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let feed: Feed = json(&body);
    assert_eq!(feed.title.as_deref(), Some("Renamed"));
    assert_eq!(feed.refresh_seconds, 600);

    // Refresh intervals must be at least one minute
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &path,
        Some(r#"{"refresh_seconds":10}"#),
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);

    let (code, body) = request(daemon.socket(), "DELETE", &path, None).await;
    assert_eq!(code, 204, "{body}");

    let (code, _) = request(daemon.socket(), "GET", &path, None).await;
    assert_eq!(code, 404);
    assert!(entries(&daemon, "").await.items.is_empty());
}

#[tokio::test]
async fn pages_the_feed_list() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    add(&daemon, &site.url("/feed.xml")).await;
    add(&daemon, &page_site().await.url("/feed.xml")).await;

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{FEEDS_PATH}?limit=1"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let page: Page<Feed> = json(&body);
    assert_eq!(page.items.len(), 1);
    let cursor = page.next.expect("a page which is not the last");

    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{FEEDS_PATH}?limit=1&cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let rest: Page<Feed> = json(&body);
    assert_eq!(rest.items.len(), 1);
    assert_ne!(rest.items[0].id, page.items[0].id);
    assert_eq!(rest.next, None);
}

#[tokio::test]
async fn refuses_a_cursor_made_for_another_list() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    add(&daemon, &site.url("/feed.xml")).await;

    add(&daemon, &page_site().await.url("/feed.xml")).await;

    let (_, body) = request(
        daemon.socket(),
        "GET",
        &format!("{FEEDS_PATH}?limit=1"),
        None,
    )
    .await;
    let page: Page<Feed> = json(&body);
    let cursor = page.next.expect("a page which is not the last");

    // Cursors belong to one list and sort order
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{ENTRIES_PATH}?cursor={cursor}"),
        None,
    )
    .await;
    assert_eq!(code, 400, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

#[tokio::test]
async fn reports_a_feed_and_an_entry_which_do_not_exist() {
    let daemon = Daemon::start().await;

    for path in [reader::feed_path(7), reader::entry_path(7)] {
        let (code, body) = request(daemon.socket(), "GET", &path, None).await;
        assert_eq!(code, 404, "{path}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound);
    }
}

#[tokio::test]
async fn fetches_a_feed_as_the_user_asked() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;

    let request_body = serde_json::json!({
        "url": site.url("/feed.xml"),
        "timeout_seconds": 7,
        "user_agent": "Jane's reader",
    })
    .to_string();
    let (code, body) = request(daemon.socket(), "POST", FEEDS_PATH, Some(&request_body)).await;
    assert_eq!(code, 201, "{body}");
    let refreshed: Refreshed = json(&body);
    assert_eq!(refreshed.feed.timeout_seconds, 7);
    assert_eq!(refreshed.feed.user_agent.as_deref(), Some("Jane's reader"));
    assert_eq!(site.user_agents(), ["Jane's reader"]);

    // Feeds inherit the node's user agent and timeout
    let plain = add(&daemon, &site.url("/feed.xml")).await;
    assert_eq!(
        plain.feed.id, refreshed.feed.id,
        "the feed is already stored"
    );
    let other = add(&daemon, &page_site().await.url("/feed.xml")).await;
    assert_eq!(other.feed.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(other.feed.user_agent, None);
    assert_eq!(
        site.user_agents().last().map(String::as_str),
        Some(DEFAULT_USER_AGENT)
    );
}

#[tokio::test]
async fn changes_how_a_feed_is_fetched() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    let refreshed = add(&daemon, &site.url("/feed.xml")).await;
    let path = reader::feed_path(refreshed.feed.id);

    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &path,
        Some(r#"{"timeout_seconds":90,"user_agent":"another reader"}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let feed: Feed = json(&body);
    assert_eq!(feed.timeout_seconds, 90);
    assert_eq!(feed.user_agent.as_deref(), Some("another reader"));

    // The next fetch uses the updated settings
    let (code, body) = request(
        daemon.socket(),
        "POST",
        &reader::refresh_path(feed.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        site.user_agents().last().map(String::as_str),
        Some("another reader")
    );

    // Null restores the default user agent
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        &path,
        Some(r#"{"user_agent":null}"#),
    )
    .await;
    assert_eq!(code, 200, "{body}");
    let feed: Feed = json(&body);
    assert_eq!(feed.user_agent, None);
    assert_eq!(feed.timeout_seconds, 90, "the timeout stays");

    let (code, body) = request(
        daemon.socket(),
        "POST",
        &reader::refresh_path(feed.id),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert_eq!(
        site.user_agents().last().map(String::as_str),
        Some(DEFAULT_USER_AGENT)
    );
}

#[tokio::test]
async fn refuses_a_timeout_outside_the_schema() {
    let daemon = Daemon::start().await;
    let site = site(RSS).await;
    let refreshed = add(&daemon, &site.url("/feed.xml")).await;

    for timeout in [0, 601] {
        let (code, body) = request(
            daemon.socket(),
            "PATCH",
            &reader::feed_path(refreshed.feed.id),
            Some(&format!(
                "{}",
                serde_json::json!({ "timeout_seconds": timeout })
            )),
        )
        .await;
        assert_eq!(code, 400, "{timeout}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::Invalid, "{timeout}");
    }
}
