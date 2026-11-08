//! Tests for [`lockbox_store::local::feeds`].

use crate::common::Temp;
use crate::fixtures::{feed, item, list, open, poll, titles};
use lockbox_store::{
    Error,
    local::{
        Visibility,
        entries::{self, Filter},
        feeds::{self, Change, Kind, NewFeed, Poll, Source},
    },
};

#[tokio::test]
async fn adds_a_feed() {
    let temp = Temp::new("add-feed");
    let db = open(&temp);

    let stored = feeds::add(
        &db,
        NewFeed {
            site_url: Some("https://example.com/".to_owned()),
            title: Some("Example".to_owned()),
            ..NewFeed::new("https://example.com/feed.xml")
        },
        100,
    )
    .await
    .expect("add feed");

    assert_eq!(stored.url, "https://example.com/feed.xml");
    assert_eq!(stored.site_url.as_deref(), Some("https://example.com/"));
    assert_eq!(stored.title.as_deref(), Some("Example"));
    assert_eq!(stored.kind, Kind::Rss);
    assert_eq!(stored.poll_seconds, 3600);
    assert_eq!(stored.next_poll_at, None);
    assert_eq!(stored.fetched_at, None);
    assert_eq!(stored.visibility, Visibility::Private);
    assert!(!stored.disabled);
    assert_eq!(stored.discovered_from, Source::User);
    assert_eq!(stored.created_at, 100);
    assert_eq!(stored.updated_at, 100);

    assert_eq!(
        feeds::get(&db, stored.id).await.expect("read feed"),
        Some(stored.clone())
    );
    assert_eq!(feeds::list(&db).await.expect("list feeds"), [stored]);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn adding_a_known_feed_returns_it() {
    let temp = Temp::new("known-feed");
    let db = open(&temp);

    let first = feeds::add(&db, NewFeed::new("https://example.com/feed"), 100)
        .await
        .expect("add feed");
    let second = feeds::add(
        &db,
        NewFeed {
            title: Some("Ignored".to_owned()),
            ..NewFeed::new("https://example.com/feed")
        },
        200,
    )
    .await
    .expect("add the same feed again");

    assert_eq!(first.id, second.id);
    assert_eq!(second.title, None);
    assert_eq!(second.created_at, 100);
    assert_eq!(feeds::list(&db).await.expect("list feeds").len(), 1);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn records_a_poll() {
    let temp = Temp::new("poll");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;

    let inserted = feeds::record_poll(
        &db,
        feed_id,
        Poll {
            fetched_at: 200,
            etag: Some("\"abc\"".to_owned()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
            entries: vec![item("a", Some(150)), item("b", None)],
        },
    )
    .await
    .expect("record poll");

    assert_eq!(inserted, 2);
    let stored = feeds::get(&db, feed_id)
        .await
        .expect("read feed")
        .expect("feed exists");
    assert_eq!(stored.fetched_at, Some(200));
    assert_eq!(stored.etag.as_deref(), Some("\"abc\""));
    assert_eq!(
        stored.last_modified.as_deref(),
        Some("Mon, 01 Jan 2024 00:00:00 GMT")
    );
    assert_eq!(stored.next_poll_at, Some(200 + 3600));
    assert_eq!(stored.last_error, None);
    assert_eq!(stored.updated_at, 200);

    // Newest first, with the fetch time standing in for a missing date
    let page = entries::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list entries");
    assert_eq!(titles(&page), ["Entry b", "Entry a"]);
    assert_eq!(page.items[0].sort_at, 200);
    assert_eq!(page.items[1].sort_at, 150);
    assert_eq!(page.next, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn a_second_poll_skips_known_entries() {
    let temp = Temp::new("known-items");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;

    let first = poll(
        &db,
        feed_id,
        200,
        vec![item("a", Some(150)), item("b", Some(150))],
    )
    .await;
    let second = poll(
        &db,
        feed_id,
        300,
        vec![item("a", Some(150)), item("c", Some(150))],
    )
    .await;

    assert_eq!(first, 2);
    assert_eq!(second, 1);
    // Equal dates fall back to the id, newest row first
    assert_eq!(
        list(&db, Filter::default()).await,
        ["Entry c", "Entry b", "Entry a"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn records_a_failed_poll() {
    let temp = Temp::new("failed-poll");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;
    feeds::record_poll(
        &db,
        feed_id,
        Poll {
            fetched_at: 200,
            etag: Some("\"abc\"".to_owned()),
            last_modified: None,
            entries: vec![item("a", Some(150))],
        },
    )
    .await
    .expect("record poll");

    feeds::record_failure(&db, feed_id, "connection refused", 500)
        .await
        .expect("record failure");

    let stored = feeds::get(&db, feed_id)
        .await
        .expect("read feed")
        .expect("feed exists");
    assert_eq!(stored.fetched_at, Some(500));
    assert_eq!(stored.next_poll_at, Some(500 + 3600));
    assert_eq!(stored.last_error.as_deref(), Some("connection refused"));
    assert_eq!(stored.etag.as_deref(), Some("\"abc\""));
    assert_eq!(stored.updated_at, 500);
    assert_eq!(list(&db, Filter::default()).await, ["Entry a"]);

    // A later success clears the error and replaces the validators
    poll(&db, feed_id, 600, vec![]).await;
    let stored = feeds::get(&db, feed_id)
        .await
        .expect("read feed")
        .expect("feed exists");
    assert_eq!(stored.last_error, None);
    assert_eq!(stored.etag, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn a_poll_of_a_missing_feed_fails() {
    let temp = Temp::new("missing-feed");
    let db = open(&temp);

    let poll = Poll {
        fetched_at: 100,
        etag: None,
        last_modified: None,
        entries: vec![item("a", None)],
    };
    let error = feeds::record_poll(&db, 999, poll)
        .await
        .expect_err("no such feed");
    assert!(matches!(error, Error::NotFound), "{error}");

    let error = feeds::record_failure(&db, 999, "gone", 100)
        .await
        .expect_err("no such feed");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn changes_feed_settings() {
    let temp = Temp::new("settings");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    poll(&db, feed_id, 100, vec![]).await;

    let changed = feeds::update(
        &db,
        feed_id,
        Change {
            url: Some("https://example.com/new-feed".to_owned()),
            site_url: Some("https://example.com/".to_owned()),
            title: Some("Example".to_owned()),
            kind: Some(Kind::Atom),
            poll_seconds: Some(600),
            visibility: Some(Visibility::Public),
            disabled: Some(true),
        },
        200,
    )
    .await
    .expect("change settings");

    assert_eq!(changed.url, "https://example.com/new-feed");
    assert_eq!(changed.site_url.as_deref(), Some("https://example.com/"));
    assert_eq!(changed.title.as_deref(), Some("Example"));
    assert_eq!(changed.kind, Kind::Atom);
    assert_eq!(changed.poll_seconds, 600);
    assert_eq!(changed.next_poll_at, Some(200 + 600), "the poll moves");
    assert_eq!(changed.visibility, Visibility::Public);
    assert!(changed.disabled);
    assert_eq!(changed.updated_at, 200);

    // Only the fields that are named move
    let changed = feeds::update(
        &db,
        feed_id,
        Change {
            title: Some("Renamed".to_owned()),
            ..Change::default()
        },
        300,
    )
    .await
    .expect("rename");
    assert_eq!(changed.title.as_deref(), Some("Renamed"));
    assert_eq!(changed.url, "https://example.com/new-feed");
    assert_eq!(changed.poll_seconds, 600);

    // A feed that was never polled has no next poll to move
    let fresh = feed(&db, "https://fresh.example/feed", 10).await;
    let changed = feeds::update(
        &db,
        fresh,
        Change {
            poll_seconds: Some(300),
            ..Change::default()
        },
        50,
    )
    .await
    .expect("set interval");
    assert_eq!(changed.next_poll_at, None);

    // The schema keeps the interval at a minute or more
    let error = feeds::update(
        &db,
        feed_id,
        Change {
            poll_seconds: Some(10),
            ..Change::default()
        },
        400,
    )
    .await
    .expect_err("below a minute");
    assert!(error.to_string().contains("poll_seconds"), "{error}");

    let error = feeds::update(&db, 9_999, Change::default(), 400)
        .await
        .expect_err("no such feed");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}
