//! Tests for [`lockbox_store::local::bookmarks`].

use crate::common::Temp;
use crate::fixtures::{bookmark, listed, open};
use lockbox_store::{
    Error,
    local::{bookmarks, folders, tags, topics},
};
use lockbox_types::{
    bookmark::{BookmarkChange, Filter, NewBookmark, Scope},
    folder::NewFolder,
    visibility::Visibility,
};

#[tokio::test]
async fn saves_a_bookmark() {
    let temp = Temp::new("save-bookmark");
    let db = open(&temp);

    let saved = bookmarks::create(
        &db,
        NewBookmark {
            note: Some("read later".to_owned()),
            ..NewBookmark::new("https://example.com/a", "Example", Visibility::Public)
        },
        100,
    )
    .await
    .expect("save bookmark");
    assert!(saved.created);
    let saved = saved.bookmark;

    assert_eq!(saved.url, "https://example.com/a");
    assert_eq!(saved.canonical_url, "https://example.com/a");
    assert_eq!(saved.title, "Example");
    assert_eq!(saved.note.as_deref(), Some("read later"));
    assert_eq!(saved.folder_id, None);
    assert_eq!(saved.position, 0);
    assert_eq!(saved.entry_id, None);
    assert_eq!(saved.created_at, 100);
    assert_eq!(saved.updated_at, 100);
    assert_eq!(saved.visibility, Visibility::Public);
    assert!(saved.topics.is_empty());
    assert_eq!(
        bookmarks::get(&db, saved.id).await.expect("read bookmark"),
        Some(saved)
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn saves_the_topics_it_was_given() {
    let temp = Temp::new("save-topic");
    let db = open(&temp);

    let saved = bookmarks::create(
        &db,
        NewBookmark {
            topics: vec!["Rust".to_owned(), "databases".to_owned()],
            ..NewBookmark::new("https://example.com/a", "Example", Visibility::Public)
        },
        100,
    )
    .await
    .expect("save bookmark")
    .bookmark;
    let names: Vec<&str> = saved
        .topics
        .iter()
        .map(|topic| topic.name.as_str())
        .collect();
    assert_eq!(names, ["databases", "Rust"]);

    let listed = bookmarks::list(&db, &Filter::default(), None, usize::MAX)
        .await
        .expect("list bookmarks");
    assert_eq!(listed.items[0].topics.len(), 2);
    assert_eq!(topics::list(&db).await.expect("list topics").len(), 2);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn keeps_one_bookmark_per_url() {
    let temp = Temp::new("one-bookmark");
    let db = open(&temp);

    let first = bookmarks::create(
        &db,
        NewBookmark::new("https://example.com/a", "Example", Visibility::Public),
        100,
    )
    .await
    .expect("save bookmark");
    assert!(first.created);
    let first = first.bookmark;
    let same = bookmarks::create(
        &db,
        NewBookmark {
            topics: vec!["Other".to_owned()],
            ..NewBookmark::new("https://example.com/a", "Other", Visibility::Public)
        },
        200,
    )
    .await
    .expect("save the same link again");
    assert!(!same.created);
    let same = same.bookmark;

    assert_eq!(same.id, first.id);
    assert_eq!(same.title, "Example");
    assert_eq!(same.created_at, 100);
    assert!(same.topics.is_empty(), "a second save does not retag");

    // The canonical address decides, not the one that was typed
    let canonical = bookmarks::create(
        &db,
        NewBookmark {
            canonical_url: "https://example.com/a".to_owned(),
            ..NewBookmark::new(
                "https://example.com/a?utm_source=feed",
                "Same",
                Visibility::Public,
            )
        },
        300,
    )
    .await
    .expect("save the canonical address");
    assert!(!canonical.created);
    assert_eq!(canonical.bookmark.id, first.id);
    assert_eq!(listed(&db, Filter::default()).await.len(), 1);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn keeps_the_order_inside_a_folder() {
    let temp = Temp::new("folder-order");
    let db = open(&temp);
    let folder = folders::create(&db, NewFolder::new(None, "Reading"), 10)
        .await
        .expect("create folder")
        .id;
    let a = bookmark(&db, "https://a.example", Some(folder), 20).await;
    bookmark(&db, "https://b.example", Some(folder), 21).await;
    let c = bookmark(&db, "https://c.example", Some(folder), 22).await;

    let filter = Filter {
        folder: Scope::Folder(folder),
        ..Filter::default()
    };
    let page = bookmarks::list(&db, &filter, None, usize::MAX)
        .await
        .expect("list bookmarks");
    assert_eq!(
        bookmark_titles(&page),
        [
            "https://a.example",
            "https://b.example",
            "https://c.example"
        ]
    );

    bookmarks::place(&db, c, Some(folder), 0, 30)
        .await
        .expect("move to the front");
    let page = bookmarks::list(&db, &filter, None, usize::MAX)
        .await
        .expect("list bookmarks");
    assert_eq!(
        bookmark_titles(&page),
        [
            "https://c.example",
            "https://a.example",
            "https://b.example"
        ]
    );
    let positions: Vec<i64> = page
        .items
        .iter()
        .map(|bookmark| bookmark.position)
        .collect();
    assert_eq!(positions, [0, 1, 2], "the folder is renumbered");

    // Past the end lands at the end
    bookmarks::place(&db, a, Some(folder), 99, 40)
        .await
        .expect("move to the end");
    assert_eq!(
        listed(&db, filter).await,
        [
            "https://c.example",
            "https://b.example",
            "https://a.example"
        ]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn moves_a_bookmark_between_folders() {
    let temp = Temp::new("move-bookmark");
    let db = open(&temp);
    let one = folders::create(&db, NewFolder::new(None, "One"), 10)
        .await
        .expect("create folder")
        .id;
    let two = folders::create(&db, NewFolder::new(None, "Two"), 10)
        .await
        .expect("create folder")
        .id;
    let a = bookmark(&db, "https://a.example", Some(one), 20).await;
    bookmark(&db, "https://b.example", Some(one), 21).await;
    bookmark(&db, "https://c.example", Some(two), 22).await;

    bookmarks::place(&db, a, Some(two), 0, 30)
        .await
        .expect("move between folders");

    let two_page = bookmarks::list(
        &db,
        &Filter {
            folder: Scope::Folder(two),
            ..Filter::default()
        },
        None,
        usize::MAX,
    )
    .await
    .expect("list bookmarks");
    assert_eq!(
        bookmark_titles(&two_page),
        ["https://a.example", "https://c.example"]
    );
    assert_eq!(
        listed(
            &db,
            Filter {
                folder: Scope::Folder(one),
                ..Filter::default()
            }
        )
        .await,
        ["https://b.example"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn pages_through_bookmarks() {
    let temp = Temp::new("page-bookmarks");
    let db = open(&temp);
    for (n, name) in ["a", "b", "c", "d", "e"].iter().enumerate() {
        bookmark(&db, &format!("https://{name}.example"), None, n as i64 * 10).await;
    }

    let first = bookmarks::list(&db, &Filter::default(), None, 2)
        .await
        .expect("first page");
    assert_eq!(
        bookmark_titles(&first),
        ["https://e.example", "https://d.example"]
    );
    let cursor = first.next.expect("next page");

    // A newer bookmark lands above the cursor, a deleted one was never served
    bookmark(&db, "https://f.example", None, 60).await;
    let all = bookmarks::list(&db, &Filter::default(), None, usize::MAX)
        .await
        .expect("list bookmarks");
    let oldest = all
        .items
        .iter()
        .find(|bookmark| bookmark.title == "https://a.example")
        .expect("the oldest bookmark")
        .id;
    bookmarks::delete(&db, oldest)
        .await
        .expect("delete bookmark");

    let second = bookmarks::list(&db, &Filter::default(), Some(cursor), 10)
        .await
        .expect("second page");
    assert_eq!(
        bookmark_titles(&second),
        ["https://c.example", "https://b.example"]
    );
    assert_eq!(second.next, None);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn filters_bookmarks() {
    let temp = Temp::new("filter-bookmarks");
    let db = open(&temp);
    let folder = folders::create(&db, NewFolder::new(None, "Reading"), 10)
        .await
        .expect("create folder")
        .id;
    bookmark(&db, "https://unfiled.example", None, 10).await;
    bookmark(&db, "https://filed.example", Some(folder), 11).await;
    let archived = bookmark(&db, "https://archived.example", None, 12).await;
    bookmarks::mark_archived(&db, archived, Some(20))
        .await
        .expect("archive");

    assert_eq!(
        listed(
            &db,
            Filter {
                folder: Scope::Unfiled,
                ..Filter::default()
            }
        )
        .await,
        ["https://archived.example", "https://unfiled.example"]
    );
    assert_eq!(
        listed(
            &db,
            Filter {
                folder: Scope::Folder(folder),
                ..Filter::default()
            }
        )
        .await,
        ["https://filed.example"]
    );
    assert_eq!(
        listed(&db, Filter::default()).await,
        [
            "https://archived.example",
            "https://filed.example",
            "https://unfiled.example"
        ]
    );
    assert_eq!(
        listed(
            &db,
            Filter {
                archived: Some(false),
                ..Filter::default()
            }
        )
        .await,
        ["https://filed.example", "https://unfiled.example"]
    );
    assert_eq!(
        listed(
            &db,
            Filter {
                archived: Some(true),
                ..Filter::default()
            }
        )
        .await,
        ["https://archived.example"]
    );
    db.close().await.expect("close database");
}

#[tokio::test]
async fn changes_a_bookmark() {
    let temp = Temp::new("change-bookmark");
    let db = open(&temp);
    let saved = bookmark(&db, "https://example.com/a", None, 10).await;

    let changed = bookmarks::update(
        &db,
        saved,
        BookmarkChange {
            title: Some("Better".to_owned()),
            note: Some(Some("A note".to_owned())),
            visibility: Some(Visibility::Private),
            archived: Some(true),
            ..BookmarkChange::default()
        },
        20,
    )
    .await
    .expect("change bookmark");
    assert_eq!(changed.title, "Better");
    assert_eq!(changed.note.as_deref(), Some("A note"));
    assert_eq!(changed.visibility, Visibility::Private);
    assert_eq!(changed.archived_at, Some(20));
    assert_eq!(changed.url, "https://example.com/a");
    assert_eq!(changed.updated_at, 20);

    // An empty note clears it
    let changed = bookmarks::update(
        &db,
        saved,
        BookmarkChange {
            note: Some(None),
            ..BookmarkChange::default()
        },
        30,
    )
    .await
    .expect("clear note");
    assert_eq!(changed.note, None);

    let error = bookmarks::update(&db, 9_999, BookmarkChange::default(), 40)
        .await
        .expect_err("no such bookmark");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn refuses_an_address_which_is_taken() {
    let temp = Temp::new("taken-address");
    let db = open(&temp);
    let one = bookmark(&db, "https://one.example", None, 10).await;
    bookmark(&db, "https://two.example", None, 11).await;

    let error = bookmarks::update(
        &db,
        one,
        BookmarkChange {
            url: Some("https://two.example".to_owned()),
            canonical_url: Some("https://two.example".to_owned()),
            ..BookmarkChange::default()
        },
        20,
    )
    .await
    .expect_err("duplicate address");
    assert!(matches!(error, Error::Conflict(_)), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn deletes_a_bookmark_and_its_topics() {
    let temp = Temp::new("delete-bookmark");
    let db = open(&temp);
    let saved = bookmark(&db, "https://example.com/a", None, 10).await;
    tags::set(&db, saved, vec!["Rust".to_owned()], 20)
        .await
        .expect("tag");

    bookmarks::delete(&db, saved)
        .await
        .expect("delete bookmark");
    assert_eq!(
        bookmarks::get(&db, saved).await.expect("read bookmark"),
        None
    );
    let links: i64 = db
        .call(|connection| {
            Ok(
                connection
                    .query_row("SELECT count(*) FROM bookmark_topic", [], |row| row.get(0))?,
            )
        })
        .await
        .expect("count links");
    assert_eq!(links, 0);
    assert_eq!(
        topics::list(&db).await.expect("list topics").len(),
        1,
        "the topic stays in the namespace"
    );

    let error = bookmarks::delete(&db, saved).await.expect_err("gone");
    assert!(matches!(error, Error::NotFound), "{error}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn keeps_the_visibility_it_was_given() {
    let temp = Temp::new("visibility");
    let db = open(&temp);

    let private = bookmarks::create(
        &db,
        NewBookmark::new("https://a.example", "A", Visibility::Private),
        10,
    )
    .await
    .expect("save bookmark")
    .bookmark;
    assert_eq!(private.visibility, Visibility::Private);

    let public = bookmarks::create(
        &db,
        NewBookmark::new("https://b.example", "B", Visibility::Public),
        20,
    )
    .await
    .expect("save bookmark")
    .bookmark;
    assert_eq!(public.visibility, Visibility::Public);

    // A later save does not change what is already there
    let stored = bookmarks::get(&db, private.id)
        .await
        .expect("read bookmark")
        .expect("bookmark");
    assert_eq!(stored.visibility, Visibility::Private);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn searches_every_word_literally() {
    let temp = Temp::new("search-bookmark");
    let db = open(&temp);
    bookmark(&db, "https://example.com/a", None, 10).await;
    let saved = bookmarks::list(&db, &Filter::default(), None, 10)
        .await
        .expect("list bookmarks")
        .items
        .remove(0)
        .id;
    bookmarks::update(
        &db,
        saved,
        BookmarkChange {
            title: Some("Rust and \"databases\"".to_owned()),
            ..BookmarkChange::default()
        },
        20,
    )
    .await
    .expect("rename");

    for query in ["Rust databases", "\"databases\"", "rust"] {
        let page = bookmarks::list(
            &db,
            &Filter {
                query: Some(query.to_owned()),
                ..Filter::default()
            },
            None,
            10,
        )
        .await
        .expect("search");
        assert_eq!(page.items.len(), 1, "{query}");
    }

    // Punctuation cannot turn into an FTS syntax error
    let page = bookmarks::list(
        &db,
        &Filter {
            query: Some("\"".to_owned()),
            ..Filter::default()
        },
        None,
        10,
    )
    .await
    .expect("a quote is a term");
    assert!(page.items.is_empty());
    db.close().await.expect("close database");
}

fn bookmark_titles(page: &bookmarks::Page) -> Vec<&str> {
    page.items
        .iter()
        .map(|bookmark| bookmark.title.as_str())
        .collect()
}
