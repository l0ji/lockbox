//! Tests for the reader half of [`lockbox_store::local`].

mod common;

use common::Temp;
use lockbox_store::{
    Db, Error,
    local::{
        self, Visibility,
        entries::{self, Filter, NewEntry},
        feeds::{self, Change, Kind, NewFeed, Poll, Source},
    },
};

/// Open a local database inside the temporary directory.
fn open(temp: &Temp) -> Db {
    local::open(&temp.db()).expect("open local database")
}

/// Add a feed and return its id.
async fn feed(db: &Db, url: &str, now: i64) -> i64 {
    feeds::add(db, NewFeed::new(url), now)
        .await
        .expect("add feed")
        .id
}

/// An entry from a feed, titled after its source.
fn item(source: &str, published_at: Option<i64>) -> NewEntry {
    NewEntry {
        source: source.to_owned(),
        url: Some(format!("https://example.com/{source}")),
        canonical_url: None,
        title: format!("Entry {source}"),
        author: None,
        summary: None,
        body: None,
        truncated: false,
        published_at,
    }
}

/// Record a poll at `now` that found `entries`.
async fn poll(db: &Db, id: i64, now: i64, entries: Vec<NewEntry>) -> usize {
    feeds::record_poll(
        db,
        id,
        Poll {
            fetched_at: now,
            etag: None,
            last_modified: None,
            entries,
        },
    )
    .await
    .expect("record poll")
}

/// Titles of a page, in order.
fn titles(page: &entries::Page) -> Vec<&str> {
    page.items
        .iter()
        .map(|entry| entry.title.as_str())
        .collect()
}

/// Titles of every entry a filter matches.
async fn list(db: &Db, filter: Filter) -> Vec<String> {
    entries::list(db, &filter, None, usize::MAX)
        .await
        .expect("list entries")
        .items
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}

/// Id of the entry titled `Entry {source}`.
async fn id(db: &Db, source: &str) -> i64 {
    let wanted = format!("Entry {source}");
    let page = entries::list(db, &Filter::default(), None, usize::MAX)
        .await
        .expect("list entries");
    page.items
        .into_iter()
        .find(|entry| entry.title == wanted)
        .expect("entry exists")
        .id
}

/// Body and truncation flag of one entry.
async fn body(db: &Db, id: i64) -> (String, bool) {
    db.call(move |connection| {
        Ok(connection.query_row(
            "SELECT body, truncated FROM entry WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?)
    })
    .await
    .expect("read body")
}

/// Whether `text` is a guid the store made.
fn is_guid(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Delete one entry by source, which no store query does yet.
async fn remove(db: &Db, source: &str) {
    let source = source.to_owned();
    db.call(move |connection| {
        connection.execute("DELETE FROM entry WHERE source = ?1", [&source])?;
        Ok(())
    })
    .await
    .expect("remove entry");
}

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
async fn pages_through_the_reading_list() {
    let temp = Temp::new("paging");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    let items = (1..=5)
        .map(|number| item(&number.to_string(), Some(number * 10)))
        .collect();
    poll(&db, feed_id, 100, items).await;

    let mut seen = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let page = entries::list(&db, &Filter::default(), cursor, 2)
            .await
            .expect("page");
        pages += 1;
        seen.extend(page.items.into_iter().map(|entry| entry.title));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }

    assert_eq!(
        seen,
        ["Entry 5", "Entry 4", "Entry 3", "Entry 2", "Entry 1"]
    );
    assert_eq!(pages, 3);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn the_cursor_survives_inserts_and_deletes() {
    let temp = Temp::new("cursor");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    let items = (1..=5)
        .map(|number| item(&number.to_string(), Some(number * 10)))
        .collect();
    poll(&db, feed_id, 100, items).await;

    let first = entries::list(&db, &Filter::default(), None, 2)
        .await
        .expect("first page");
    assert_eq!(titles(&first), ["Entry 5", "Entry 4"]);
    let cursor = first.next.expect("next page");

    // A newer item lands above the cursor, a missing one was never served
    poll(
        &db,
        feed_id,
        200,
        vec![item("6", Some(45)), item("7", Some(5))],
    )
    .await;
    remove(&db, "3").await;

    let second = entries::list(&db, &Filter::default(), Some(cursor), 10)
        .await
        .expect("second page");
    assert_eq!(titles(&second), ["Entry 2", "Entry 1", "Entry 7"]);
    assert_eq!(second.next, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn filters_by_feed_and_read_state() {
    let temp = Temp::new("filter-feed");
    let db = open(&temp);
    let first = feed(&db, "https://one.example/feed", 10).await;
    let second = feed(&db, "https://two.example/feed", 10).await;
    poll(
        &db,
        first,
        100,
        vec![
            item("a", Some(10)),
            item("b", Some(20)),
            item("c", Some(30)),
        ],
    )
    .await;
    poll(&db, second, 100, vec![item("d", Some(40))]).await;

    entries::mark_read(&db, id(&db, "b").await, Some(150))
        .await
        .expect("mark read");

    assert_eq!(
        list(
            &db,
            Filter {
                feed_id: Some(first),
                ..Filter::default()
            }
        )
        .await,
        ["Entry c", "Entry b", "Entry a"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                read: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["Entry d", "Entry c", "Entry a"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                read: Some(true),
                ..Filter::default()
            }
        )
        .await,
        ["Entry b"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                feed_id: Some(first),
                read: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["Entry c", "Entry a"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn filters_by_star_and_archived() {
    let temp = Temp::new("filter-state");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    poll(
        &db,
        feed_id,
        100,
        vec![item("a", Some(10)), item("b", Some(20))],
    )
    .await;

    entries::mark_starred(&db, id(&db, "a").await, Some(150))
        .await
        .expect("mark starred");
    entries::mark_archived(&db, id(&db, "b").await, Some(150))
        .await
        .expect("mark archived");

    assert_eq!(
        list(
            &db,
            Filter {
                starred: Some(true),
                ..Filter::default()
            }
        )
        .await,
        ["Entry a"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                starred: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["Entry b"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                archived: Some(true),
                ..Filter::default()
            }
        )
        .await,
        ["Entry b"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                archived: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["Entry a"]
    );
    assert_eq!(
        list(
            &db,
            Filter {
                read: Some(false),
                archived: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["Entry a"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn finds_entries_by_text() {
    let temp = Temp::new("search");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;

    let mut runtime = item("a", Some(10));
    runtime.title = "An async runtime".to_owned();
    runtime.body = Some("the borrow checker keeps you honest".to_owned());
    let mut stew = item("b", Some(20));
    stew.title = "A slow stew".to_owned();
    stew.summary = Some("Browning the meat first".to_owned());
    poll(&db, feed_id, 100, vec![runtime, stew]).await;

    let query = |text: &str| Filter {
        query: Some(text.to_owned()),
        ..Filter::default()
    };
    assert_eq!(list(&db, query("borrow")).await, ["An async runtime"]);
    assert_eq!(list(&db, query("browning")).await, ["A slow stew"]);
    assert_eq!(
        list(&db, query("runtime OR stew")).await,
        ["A slow stew", "An async runtime"]
    );
    assert!(list(&db, query("nothing")).await.is_empty());
    assert_eq!(
        list(&db, query("")).await.len(),
        2,
        "an empty query matches"
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn the_search_index_follows_edits_and_deletes() {
    let temp = Temp::new("search-sync");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    let mut runtime = item("a", Some(10));
    runtime.title = "An async runtime".to_owned();
    poll(&db, feed_id, 100, vec![runtime]).await;

    let entry_id = entries::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list entries")
        .items[0]
        .id;
    db.call(move |connection| {
        connection.execute(
            "UPDATE entry SET title = 'A slow stew' WHERE id = ?1",
            [entry_id],
        )?;
        Ok(())
    })
    .await
    .expect("rename entry");

    let query = |text: &str| Filter {
        query: Some(text.to_owned()),
        ..Filter::default()
    };
    assert!(list(&db, query("async")).await.is_empty());
    assert_eq!(list(&db, query("stew")).await, ["A slow stew"]);

    db.call(move |connection| {
        connection.execute("DELETE FROM entry WHERE id = ?1", [entry_id])?;
        Ok(())
    })
    .await
    .expect("delete entry");
    assert!(list(&db, query("stew")).await.is_empty());
    db.close().await.expect("close database");
}

#[tokio::test]
async fn marks_reader_state() {
    let temp = Temp::new("marks");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;
    poll(&db, feed_id, 100, vec![item("a", Some(10))]).await;
    let entry_id = id(&db, "a").await;

    entries::mark_read(&db, entry_id, Some(300))
        .await
        .expect("mark read");
    entries::mark_starred(&db, entry_id, Some(400))
        .await
        .expect("mark starred");

    let page = entries::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list entries");
    assert_eq!(page.items[0].read_at, Some(300));
    assert_eq!(page.items[0].starred_at, Some(400));
    assert_eq!(page.items[0].archived_at, None);

    entries::mark_read(&db, entry_id, None)
        .await
        .expect("mark unread");
    entries::mark_archived(&db, entry_id, Some(500))
        .await
        .expect("mark archived");

    let page = entries::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list entries");
    assert_eq!(page.items[0].read_at, None);
    assert_eq!(page.items[0].archived_at, Some(500));

    let error = entries::mark_read(&db, 9_999, None)
        .await
        .expect_err("no such entry");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn keeps_a_body_whole() {
    let temp = Temp::new("body");
    let db = open(&temp);
    let feed_id = feed(&db, "https://example.com/feed", 10).await;

    // Larger than a federation record may be; the reader keeps what it got
    let big = "x".repeat(3 * 1024 * 1024);
    let mut entry = item("a", Some(10));
    entry.body = Some(big.clone());
    let mut short = item("b", Some(20));
    short.body = Some("short".to_owned());
    poll(&db, feed_id, 100, vec![entry, short]).await;

    let (text, truncated) = body(&db, id(&db, "a").await).await;
    assert_eq!(text.len(), big.len());
    assert!(!truncated);

    let (text, truncated) = body(&db, id(&db, "b").await).await;
    assert_eq!(text, "short");
    assert!(!truncated);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn gives_every_entry_its_own_guid() {
    let temp = Temp::new("guid");
    let db = open(&temp);
    let one = feed(&db, "https://one.example/feed", 10).await;
    let two = feed(&db, "https://two.example/feed", 10).await;

    // Two feeds using the same identifier are still two different entries
    poll(&db, one, 100, vec![item("same", Some(10))]).await;
    poll(&db, two, 100, vec![item("same", Some(10))]).await;

    let page = entries::list(&db, &Filter::default(), None, usize::MAX)
        .await
        .expect("list entries");
    assert_eq!(page.items.len(), 2);
    let guids: Vec<&str> = page.items.iter().map(|entry| entry.guid.as_str()).collect();
    for guid in &guids {
        assert!(is_guid(guid), "{guid}");
    }
    assert_ne!(guids[0], guids[1]);

    // The feed's identifier is kept beside the guid and still dedupes
    let first = page.items[0].id;
    let source: String = db
        .call(move |connection| {
            Ok(
                connection.query_row("SELECT source FROM entry WHERE id = ?1", [first], |row| {
                    row.get(0)
                })?,
            )
        })
        .await
        .expect("read source");
    assert_eq!(source, "same");

    assert_eq!(poll(&db, one, 200, vec![item("same", Some(10))]).await, 0);
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
