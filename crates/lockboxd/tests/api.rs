//! Tests of the local API's node and reader routes.

mod common;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{Router, routing::get};
use common::{Daemon, Site, json, request};
use lockbox_core::settings::Settings;
use lockbox_proto::{
    PROTOCOL_VERSION,
    api::{
        ApiError, Code, Page,
        node::{self, SETTINGS_PATH},
        reader::{self, DEFAULT_USER_AGENT, ENTRIES_PATH, FEEDS_PATH},
    },
};
use lockbox_store::local::settings;
use lockbox_types::{
    entry::{Entry, EntrySummary},
    feed::{DEFAULT_TIMEOUT_SECONDS, Feed, Kind, Refreshed},
    node::{Health, Status},
};

/// A feed document with two items, the first of which has a body.
const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:content="http://purl.org/rss/1.0/modules/content/">
  <channel>
    <title>Example blog</title>
    <link>https://example.com/</link>
    <item>
      <title>First post</title>
      <link>https://example.com/first</link>
      <guid>first</guid>
      <description>Short summary</description>
      <content:encoded><![CDATA[<p>words</p>]]></content:encoded>
      <pubDate>Sun, 06 Sep 2009 16:20:00 +0000</pubDate>
    </item>
    <item>
      <title>Second post</title>
      <link>https://example.com/second</link>
      <guid>second</guid>
      <pubDate>Mon, 07 Sep 2009 16:20:00 +0000</pubDate>
    </item>
  </channel>
</rss>"#;

/// The same feed after its author added one more item.
const RSS_UPDATED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Example blog</title>
    <link>https://example.com/</link>
    <item>
      <title>First post</title>
      <link>https://example.com/first</link>
      <guid>first</guid>
      <pubDate>Sun, 06 Sep 2009 16:20:00 +0000</pubDate>
    </item>
    <item>
      <title>Second post</title>
      <link>https://example.com/second</link>
      <guid>second</guid>
      <pubDate>Mon, 07 Sep 2009 16:20:00 +0000</pubDate>
    </item>
    <item>
      <title>Third post</title>
      <link>https://example.com/third</link>
      <guid>third</guid>
      <pubDate>Tue, 08 Sep 2009 16:20:00 +0000</pubDate>
    </item>
  </channel>
</rss>"#;

/// A page which advertises `/feed.xml`.
const PAGE: &str = r#"<!doctype html>
<html>
  <head>
    <title>Example</title>
    <link rel="alternate" type="application/rss+xml" href="/feed.xml">
  </head>
  <body>words</body>
</html>"#;

/// Read the settings over the API.
async fn fetch(daemon: &Daemon) -> Settings {
    let (code, body) = request(daemon.socket(), "GET", SETTINGS_PATH, None).await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

/// Change settings over the API.
async fn patch(daemon: &Daemon, change: &str) -> Settings {
    let (code, body) = request(daemon.socket(), "PATCH", SETTINGS_PATH, Some(change)).await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

/// A site which serves one feed document.
async fn site(board: &'static str) -> Site {
    Site::start(Router::new().route("/feed.xml", get(move || async move { board }))).await
}

/// A site which serves a page with the feed of [`site`] linked from it.
async fn page_site() -> Site {
    Site::start(
        Router::new()
            .route("/", get(|| async { PAGE }))
            .route("/feed.xml", get(|| async { RSS })),
    )
    .await
}

/// Add the feed at `url` and answer with the refresh it came with.
async fn add(daemon: &Daemon, url: &str) -> Refreshed {
    let request_body = serde_json::json!({ "url": url }).to_string();
    let (code, body) = request(daemon.socket(), "POST", FEEDS_PATH, Some(&request_body)).await;
    assert_eq!(code, 201, "{body}");
    json(&body)
}

/// The entries of one reading list request.
async fn entries(daemon: &Daemon, query: &str) -> Page<EntrySummary> {
    let (code, body) = request(
        daemon.socket(),
        "GET",
        &format!("{ENTRIES_PATH}{query}"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    json(&body)
}

/// Titles of one reading list request.
async fn titles(daemon: &Daemon, query: &str) -> Vec<String> {
    entries(daemon, query)
        .await
        .items
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}

#[tokio::test]
async fn answers_a_health_check() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "GET", node::HEALTH_PATH, None).await;

    assert_eq!(code, 200, "{body}");
    let health: Health = json(&body);
    assert!(health.ok);
}

#[tokio::test]
async fn reports_the_node() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "GET", node::STATUS_PATH, None).await;

    assert_eq!(code, 200, "{body}");
    let status: Status = json(&body);
    assert_eq!(status.protocol_version, PROTOCOL_VERSION);
    assert_eq!(status.software_version, env!("CARGO_PKG_VERSION"));
    assert!(!status.node_id.is_empty());
    // The encoded key and the hex name the same identity
    assert!(
        status.public_key.starts_with("lbox1"),
        "{}",
        status.public_key
    );
    assert_eq!(
        lockbox_proto::key::decode_public(&status.public_key).expect("encoded public key"),
        unhex(&status.node_id),
    );

    let (_, again) = request(daemon.socket(), "GET", node::STATUS_PATH, None).await;
    let again: Status = json(&again);
    assert_eq!(again.node_id, status.node_id);
}

#[tokio::test]
async fn reports_routes_which_do_not_exist() {
    let daemon = Daemon::start().await;

    for path in ["/api/health", "/api/status", "/api/node", "/api/reader"] {
        let (code, body) = request(daemon.socket(), "GET", path, None).await;
        assert_eq!(code, 404, "{path}: {body}");
        let error: ApiError = json(&body);
        assert_eq!(error.code, Code::NotFound, "{path}");
    }
}

#[tokio::test]
async fn serves_the_default_settings() {
    let daemon = Daemon::start().await;
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn changes_one_setting() {
    let daemon = Daemon::start().await;

    let expected = Settings {
        trust_depth: 3,
        ..Settings::default()
    };
    let changed = patch(&daemon, r#"{"trust_depth":3}"#).await;
    assert_eq!(changed, expected);
    assert_eq!(fetch(&daemon).await, expected);

    // Only the value which changed is written down
    let stored = daemon
        .db()
        .call(|connection| settings::load(connection))
        .await
        .expect("read the settings table");
    assert_eq!(
        stored,
        HashMap::from([("trust_depth".to_owned(), "3".to_owned())])
    );
}

#[tokio::test]
async fn keeps_the_settings_when_nothing_is_named() {
    let daemon = Daemon::start().await;

    patch(&daemon, "{}").await;
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn clears_a_setting_sent_as_null() {
    let daemon = Daemon::start().await;

    let set = patch(&daemon, r#"{"retention_days":30}"#).await;
    assert_eq!(set.retention_days, Some(30));

    let cleared = patch(&daemon, r#"{"retention_days":null}"#).await;
    assert_eq!(cleared.retention_days, None);
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_setting_it_does_not_know() {
    let daemon = Daemon::start().await;
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        SETTINGS_PATH,
        Some(r#"{"trust":3}"#),
    )
    .await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
    assert!(error.error.contains("trust"), "{error:?}");
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_setting_of_the_wrong_type() {
    let daemon = Daemon::start().await;
    let (code, body) = request(
        daemon.socket(),
        "PATCH",
        SETTINGS_PATH,
        Some(r#"{"trust_depth":"two"}"#),
    )
    .await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
    assert_eq!(fetch(&daemon).await, Settings::default());
}

#[tokio::test]
async fn refuses_a_body_which_is_not_json() {
    let daemon = Daemon::start().await;
    let (code, body) = request(daemon.socket(), "PATCH", SETTINGS_PATH, Some("{")).await;

    assert_eq!(code, 422, "{body}");
    let error: ApiError = json(&body);
    assert_eq!(error.code, Code::Invalid);
}

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

    // The site is gone, so the next refresh fails and says so on the feed
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

    // The feed filter names the one feed which brought them
    assert_eq!(
        entries(&daemon, &format!("?feed={}", refreshed.feed.id))
            .await
            .items
            .len(),
        3
    );
    assert_eq!(entries(&daemon, "?feed=999").await.items.len(), 0);

    // Text search goes through the index
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

    // Reading it takes it out of the unread list
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

    // Starring it puts it in the starred list
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

    // The interval is what the schema allows, no less than a minute
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

    // A cursor names the list and the order it was made for
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

    // A feed which names no user agent and no timeout gets the node's own
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

    // The next fetch uses what the user asked for
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

    // Sending a null returns the feed to the user agent of this build
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

/// Bytes which a hex string names.
fn unhex(text: &str) -> [u8; 32] {
    assert_eq!(text.len(), 64, "{text}");
    let mut bytes = [0u8; 32];
    for (index, pair) in text.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(pair).expect("hex pair");
        bytes[index] = u8::from_str_radix(pair, 16).expect("hex pair");
    }
    bytes
}
