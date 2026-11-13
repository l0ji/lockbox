//! Tests for [`lockbox_store::local::feeds`].

use crate::common::Temp;
use crate::fixtures::{feed, item, list, open, refresh, titles};
use lockbox_store::{
    Error,
    local::{entries, feeds},
};
use lockbox_types::{
    entry::Filter,
    feed::{DEFAULT_TIMEOUT_SECONDS, FeedChange, Kind, NewFeed, Refresh, Source},
    visibility::Visibility,
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
    assert_eq!(stored.refresh_seconds, 3600);
    assert_eq!(stored.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(stored.user_agent, None);
    assert_eq!(stored.next_refresh_at, None);
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
    let page = feeds::list(&db, None, 10).await.expect("list feeds");
    assert_eq!(page.items, [stored]);
    assert_eq!(page.next, None);
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
    assert_eq!(
        feeds::list(&db, None, 10)
            .await
            .expect("list feeds")
            .items
            .len(),
        1
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn records_a_refresh() {
    let temp = Temp::new("refresh");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;

    let inserted = feeds::record_refresh(
        &db,
        feed_id,
        Refresh {
            fetched_at: 200,
            etag: Some("\"abc\"".to_owned()),
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
            entries: vec![item("a", Some(150)), item("b", None)],
        },
    )
    .await
    .expect("record refresh");

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
    assert_eq!(stored.next_refresh_at, Some(200 + 3600));
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
async fn a_second_refresh_skips_known_entries() {
    let temp = Temp::new("known-items");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;

    let first = refresh(
        &db,
        feed_id,
        200,
        vec![item("a", Some(150)), item("b", Some(150))],
    )
    .await;
    let second = refresh(
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
async fn records_a_failed_refresh() {
    let temp = Temp::new("failed-refresh");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 100).await;
    feeds::record_refresh(
        &db,
        feed_id,
        Refresh {
            fetched_at: 200,
            etag: Some("\"abc\"".to_owned()),
            last_modified: None,
            entries: vec![item("a", Some(150))],
        },
    )
    .await
    .expect("record refresh");

    feeds::record_failure(&db, feed_id, "connection refused", 500)
        .await
        .expect("record failure");

    let stored = feeds::get(&db, feed_id)
        .await
        .expect("read feed")
        .expect("feed exists");
    assert_eq!(stored.fetched_at, Some(500));
    assert_eq!(stored.next_refresh_at, Some(500 + 3600));
    assert_eq!(stored.last_error.as_deref(), Some("connection refused"));
    assert_eq!(stored.etag.as_deref(), Some("\"abc\""));
    assert_eq!(stored.updated_at, 500);
    assert_eq!(list(&db, Filter::default()).await, ["Entry a"]);

    // A later success clears the error and replaces the validators
    refresh(&db, feed_id, 600, vec![]).await;
    let stored = feeds::get(&db, feed_id)
        .await
        .expect("read feed")
        .expect("feed exists");
    assert_eq!(stored.last_error, None);
    assert_eq!(stored.etag, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn a_refresh_of_a_missing_feed_fails() {
    let temp = Temp::new("missing-feed");
    let db = open(&temp);

    let refresh = Refresh {
        fetched_at: 100,
        etag: None,
        last_modified: None,
        entries: vec![item("a", None)],
    };
    let error = feeds::record_refresh(&db, 999, refresh)
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
async fn lists_the_feeds_that_are_due() {
    let temp = Temp::new("due-feeds");
    let db = open(&temp);

    let never = feed(&db, "https://example.com/never.xml", 100).await;
    let overdue = feed(&db, "https://example.com/overdue.xml", 100).await;
    refresh(&db, overdue, 200, vec![]).await;
    let later = feed(&db, "https://example.com/later.xml", 100).await;
    refresh(&db, later, 4_000, vec![]).await;
    let paused = feed(&db, "https://example.com/paused.xml", 100).await;
    refresh(&db, paused, 100, vec![]).await;
    feeds::update(
        &db,
        paused,
        FeedChange {
            disabled: Some(true),
            ..FeedChange::default()
        },
        100,
    )
    .await
    .expect("pause feed");

    // 4000 is past the overdue feed's 3800 and short of the later one's 7600
    let due = feeds::due(&db, 4_000, 10).await.expect("list due feeds");
    let ids: Vec<i64> = due.iter().map(|feed| feed.id).collect();
    assert_eq!(ids, [never, overdue]);

    let due = feeds::due(&db, 4_000, 1).await.expect("list one due feed");
    assert_eq!(due.len(), 1, "the batch size bounds the pass");
    assert_eq!(due[0].id, never);

    db.close().await.expect("close database");
}

#[tokio::test]
async fn changes_feed_settings() {
    let temp = Temp::new("settings");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    refresh(&db, feed_id, 100, vec![]).await;

    let changed = feeds::update(
        &db,
        feed_id,
        FeedChange {
            url: Some("https://example.com/new-feed".to_owned()),
            site_url: Some("https://example.com/".to_owned()),
            title: Some("Example".to_owned()),
            kind: Some(Kind::Atom),
            refresh_seconds: Some(600),
            timeout_seconds: Some(5),
            user_agent: Some(Some("lockbox-test/1".to_owned())),
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
    assert_eq!(changed.refresh_seconds, 600);
    assert_eq!(changed.timeout_seconds, 5);
    assert_eq!(changed.user_agent.as_deref(), Some("lockbox-test/1"));
    assert_eq!(
        changed.next_refresh_at,
        Some(200 + 600),
        "the refresh moves"
    );
    assert_eq!(changed.visibility, Visibility::Public);
    assert!(changed.disabled);
    assert_eq!(changed.updated_at, 200);

    // Only the fields that are named move
    let changed = feeds::update(
        &db,
        feed_id,
        FeedChange {
            title: Some("Renamed".to_owned()),
            ..FeedChange::default()
        },
        300,
    )
    .await
    .expect("rename");
    assert_eq!(changed.title.as_deref(), Some("Renamed"));
    assert_eq!(changed.url, "https://example.com/new-feed");
    assert_eq!(changed.refresh_seconds, 600);

    // A feed that was never refreshed has no next refresh to move
    let fresh = feed(&db, "https://fresh.example/feed", 10).await;
    let changed = feeds::update(
        &db,
        fresh,
        FeedChange {
            refresh_seconds: Some(300),
            ..FeedChange::default()
        },
        50,
    )
    .await
    .expect("set interval");
    assert_eq!(changed.next_refresh_at, None);

    // The schema keeps the interval at a minute or more
    let error = feeds::update(
        &db,
        feed_id,
        FeedChange {
            refresh_seconds: Some(10),
            ..FeedChange::default()
        },
        400,
    )
    .await
    .expect_err("below a minute");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    assert!(error.to_string().contains("refresh_seconds"), "{error}");

    let error = feeds::update(&db, 9_999, FeedChange::default(), 400)
        .await
        .expect_err("no such feed");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn pages_the_feeds_by_title() {
    let temp = Temp::new("page-feeds");
    let db = open(&temp);

    feed(&db, "https://example.com/c", 10).await;
    let beta = feeds::add(
        &db,
        NewFeed {
            title: Some("Beta".to_owned()),
            ..NewFeed::new("https://example.com/b")
        },
        15,
    )
    .await
    .expect("add feed");
    feeds::add(
        &db,
        NewFeed {
            title: Some("alpha".to_owned()),
            ..NewFeed::new("https://example.com/a")
        },
        20,
    )
    .await
    .expect("add feed");

    // Titles order the list, and the URL stands in for a missing one
    let page = feeds::list(&db, None, 2).await.expect("list feeds");
    assert_eq!(
        page.items
            .iter()
            .map(|feed| feed.name().to_owned())
            .collect::<Vec<_>>(),
        ["alpha", "Beta"]
    );
    let cursor = page.next.expect("a page which is not the last");
    assert_eq!(cursor.name, "Beta");
    assert_eq!(cursor.id, beta.id);

    let page = feeds::list(&db, Some(cursor), 2).await.expect("next page");
    assert_eq!(page.items.len(), 1, "the page starts after the cursor");
    assert_eq!(page.next, None);
    assert_eq!(page.items[0].url, "https://example.com/c");

    // A limit of zero answers nothing at all
    let page = feeds::list(&db, None, 0).await.expect("empty page");
    assert!(page.items.is_empty());
    assert_eq!(page.next, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn removes_a_feed_and_its_entries() {
    let temp = Temp::new("remove-feed");
    let db = open(&temp);
    let removed = feed(&db, "https://example.com/feed", 10).await;
    let kept = feed(&db, "https://example.com/other", 10).await;
    refresh(&db, removed, 100, vec![item("a", Some(90))]).await;
    refresh(&db, kept, 100, vec![item("b", Some(90))]).await;

    feeds::delete(&db, removed).await.expect("remove feed");
    assert_eq!(feeds::get(&db, removed).await.expect("read feed"), None);
    assert_eq!(list(&db, Filter::default()).await, ["Entry b"]);

    let error = feeds::delete(&db, removed)
        .await
        .expect_err("a feed which is gone must not be removed twice");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn changes_how_a_feed_is_fetched() {
    let temp = Temp::new("fetch-settings");
    let db = open(&temp);
    let id = feed(&db, "https://example.com/feed", 10).await;

    let changed = feeds::update(
        &db,
        id,
        FeedChange {
            timeout_seconds: Some(90),
            user_agent: Some(Some("Jane's reader".to_owned())),
            ..FeedChange::default()
        },
        20,
    )
    .await
    .expect("change the fetch settings");
    assert_eq!(changed.timeout_seconds, 90);
    assert_eq!(changed.user_agent.as_deref(), Some("Jane's reader"));

    // A user agent sent as a null returns the feed to the default one
    let cleared = feeds::update(
        &db,
        id,
        FeedChange {
            user_agent: Some(None),
            ..FeedChange::default()
        },
        30,
    )
    .await
    .expect("clear the user agent");
    assert_eq!(cleared.user_agent, None);
    assert_eq!(cleared.timeout_seconds, 90, "the timeout is left alone");

    // The schema keeps the timeout at a second or more
    let error = feeds::update(
        &db,
        id,
        FeedChange {
            timeout_seconds: Some(0),
            ..FeedChange::default()
        },
        40,
    )
    .await
    .expect_err("a timeout of zero is refused");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn the_schema_defaults_the_fetch_settings() {
    let temp = Temp::new("fetch-defaults");
    let db = open(&temp);

    // A row which the code did not make still gets the defaults the store names
    db.call(|connection| {
        connection.execute(
            "INSERT INTO feed (url, created_at, updated_at) VALUES ('https://example.com/feed', 10, 10)",
            [],
        )?;
        Ok(())
    })
    .await
    .expect("insert a feed");

    let stored = feeds::get(&db, 1).await.expect("read feed").expect("feed");
    assert_eq!(stored.timeout_seconds, DEFAULT_TIMEOUT_SECONDS);
    assert_eq!(stored.user_agent, None);
    assert_eq!(stored.refresh_seconds, 3600);
    db.close().await.expect("close database");
}
