//! Tests for [`lockbox_store::local::entries`].

use crate::common::Temp;
use crate::fixtures::{feed, item, list, open, poll, titles};
use lockbox_store::{
    Db, Error,
    local::entries::{self, Filter},
};

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
