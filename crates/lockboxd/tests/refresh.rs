//! Tests of the refresh loop which runs beside the API.

mod common;

use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode, routing::get};
use common::{Site, TempDir};
use lockbox_core::fetch::Fetcher;
use lockbox_store::{
    Db,
    local::{
        self,
        entries::{self, Filter},
        feeds::{self, NewFeed, Refresh},
    },
};
use lockboxd::refresh;

/// A feed document with two items.
const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Example blog</title>
    <link>https://example.com/</link>
    <item>
      <title>First post</title>
      <link>https://example.com/first</link>
      <guid>first</guid>
    </item>
    <item>
      <title>Second post</title>
      <link>https://example.com/second</link>
      <guid>second</guid>
    </item>
  </channel>
</rss>"#;

/// How often the loop looks for work, short enough for a test to wait on.
const TICK: Duration = Duration::from_millis(20);

/// Open a database with one feed whose next refresh is long past.
async fn due_feed(db: &Db, url: &str) -> i64 {
    let feed = feeds::add(db, NewFeed::new(url), 1_000)
        .await
        .expect("add feed");
    // Long ago, so the next refresh is far behind the clock
    feeds::record_refresh(
        db,
        feed.id,
        Refresh {
            fetched_at: 1_000,
            etag: None,
            last_modified: None,
            entries: vec![],
        },
    )
    .await
    .expect("record refresh");
    feed.id
}

/// Start the loop on `db` and wait until the feed matches `done`.
async fn wait_for(db: &Arc<Db>, id: i64, done: impl Fn(&feeds::Feed) -> bool) -> feeds::Feed {
    let fetcher = Fetcher::new().expect("build the HTTP client");
    let task = tokio::spawn(refresh::run(Arc::clone(db), fetcher, TICK));

    let mut found = None;
    for _ in 0..250 {
        let feed = feeds::get(db, id)
            .await
            .expect("read feed")
            .expect("the feed exists");
        if done(&feed) {
            found = Some(feed);
            break;
        }
        tokio::time::sleep(TICK).await;
    }
    task.abort();
    found.expect("the loop must refresh the feed")
}

#[tokio::test]
async fn refreshes_a_feed_which_is_due() {
    let temp = TempDir::new();
    let db = Arc::new(local::open(&temp.join("local.db")).expect("open database"));
    let site = Site::start(Router::new().route("/feed.xml", get(|| async { RSS }))).await;
    let id = due_feed(&db, &site.url("/feed.xml")).await;

    let stored = wait_for(&db, id, |feed| feed.fetched_at != Some(1_000)).await;

    assert_eq!(stored.last_error, None);
    assert_eq!(
        stored.next_refresh_at,
        stored.fetched_at.map(|at| at + stored.refresh_seconds),
        "the next refresh is an interval later"
    );

    let page = entries::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list entries");
    assert_eq!(page.items.len(), 2, "the feed's items are stored");
}

#[tokio::test]
async fn records_why_a_due_feed_could_not_be_refreshed() {
    let temp = TempDir::new();
    let db = Arc::new(local::open(&temp.join("local.db")).expect("open database"));
    let site = Site::start(Router::new().route(
        "/feed.xml",
        get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
    ))
    .await;
    let id = due_feed(&db, &site.url("/feed.xml")).await;

    let stored = wait_for(&db, id, |feed| feed.last_error.is_some()).await;
    let error = stored.last_error.expect("the failure is recorded");

    assert!(error.contains("server replied 500"), "{error}");
    assert_eq!(
        stored.next_refresh_at,
        stored.fetched_at.map(|at| at + stored.refresh_seconds),
        "a failure still waits a full interval"
    );
}
