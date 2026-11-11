//! Fixtures shared by the local database tests.

use crate::common::Temp;
use lockbox_store::{
    Db,
    local::{
        self,
        bookmarks::{self, NewBookmark},
        entries, feeds,
    },
};
use lockbox_types::{
    entry::{Filter, NewEntry},
    feed::{NewFeed, Refresh, Visibility},
};

/// Open a local database inside the temporary directory.
pub(super) fn open(temp: &Temp) -> Db {
    local::open(&temp.db()).expect("open local database")
}

/// Add a feed and return its id.
pub(super) async fn feed(db: &Db, url: &str, now: i64) -> i64 {
    feeds::add(db, NewFeed::new(url), now)
        .await
        .expect("add feed")
        .id
}

/// An entry from a feed, titled after its source.
pub(super) fn item(source: &str, published_at: Option<i64>) -> NewEntry {
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

/// Record a refresh at `now` that found `entries`.
pub(super) async fn refresh(db: &Db, id: i64, now: i64, entries: Vec<NewEntry>) -> usize {
    feeds::record_refresh(
        db,
        id,
        Refresh {
            fetched_at: now,
            etag: None,
            last_modified: None,
            entries,
        },
    )
    .await
    .expect("record refresh")
}

/// Titles of an entry page, in order.
pub(super) fn titles(page: &entries::Page) -> Vec<&str> {
    page.items
        .iter()
        .map(|entry| entry.title.as_str())
        .collect()
}

/// Titles of every entry a filter matches.
pub(super) async fn list(db: &Db, filter: Filter) -> Vec<String> {
    entries::list(db, &filter, None, usize::MAX)
        .await
        .expect("list entries")
        .items
        .into_iter()
        .map(|entry| entry.title)
        .collect()
}

/// Save a bookmark, optionally in a folder, and return its id.
pub(super) async fn bookmark(db: &Db, url: &str, folder: Option<i64>, now: i64) -> i64 {
    bookmarks::create(
        db,
        NewBookmark {
            folder_id: folder,
            ..NewBookmark::new(url, url, Visibility::Public)
        },
        now,
    )
    .await
    .expect("save bookmark")
    .id
}

/// Titles of every bookmark a filter matches.
pub(super) async fn listed(db: &Db, filter: bookmarks::Filter) -> Vec<String> {
    bookmarks::list(db, &filter, None, usize::MAX)
        .await
        .expect("list bookmarks")
        .items
        .into_iter()
        .map(|bookmark| bookmark.title)
        .collect()
}
