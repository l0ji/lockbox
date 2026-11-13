//! Feed documents and request helpers shared by the API tests.

use axum::{Router, routing::get};
use lockbox_proto::api::{
    Page,
    reader::{ENTRIES_PATH, FEEDS_PATH, FOLDERS_PATH},
};
use lockbox_types::{entry::EntrySummary, feed::Refreshed, folder::Folder};

use crate::common::{Daemon, Site, json, request};

/// A feed document with two items, the first of which has a body.
pub const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
pub const RSS_UPDATED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
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
pub const PAGE: &str = r#"<!doctype html>
<html>
  <head>
    <title>Example</title>
    <link rel="alternate" type="application/rss+xml" href="/feed.xml">
  </head>
  <body>words</body>
</html>"#;

pub async fn folder(daemon: &Daemon, body: &str) -> Folder {
    let (code, body) = request(daemon.socket(), "POST", FOLDERS_PATH, Some(body)).await;
    assert_eq!(code, 201, "{body}");
    json(&body)
}

/// Serves one feed document at `/feed.xml`.
pub async fn site(board: &'static str) -> Site {
    Site::start(Router::new().route("/feed.xml", get(move || async move { board }))).await
}

pub async fn page_site() -> Site {
    Site::start(
        Router::new()
            .route("/", get(|| async { PAGE }))
            .route("/feed.xml", get(|| async { RSS })),
    )
    .await
}

pub async fn add(daemon: &Daemon, url: &str) -> Refreshed {
    let request_body = serde_json::json!({ "url": url }).to_string();
    let (code, body) = request(daemon.socket(), "POST", FEEDS_PATH, Some(&request_body)).await;
    assert_eq!(code, 201, "{body}");
    json(&body)
}

pub async fn entries(daemon: &Daemon, query: &str) -> Page<EntrySummary> {
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

pub async fn titles(daemon: &Daemon, query: &str) -> Vec<String> {
    entries(daemon, query)
        .await
        .items
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}
