//! Routes of the reader view: the feeds the user follows and the entries
//! they read.

use lockbox_types::entry::Filter;
use serde::{Deserialize, Serialize};

use super::query_string;

/// Route which lists every feed and adds one.
pub const FEEDS_PATH: &str = "/api/reader/feeds";

/// Route which reads, changes and removes one feed.
pub fn feed_path(id: i64) -> String {
    format!("{FEEDS_PATH}/{id}")
}

/// Route which fetches one feed now.
pub fn refresh_path(id: i64) -> String {
    format!("{FEEDS_PATH}/{id}/refresh")
}

/// Route which lists every entry.
pub const ENTRIES_PATH: &str = "/api/reader/entries";

/// Route which reads, changes and removes one entry.
pub fn entry_path(id: i64) -> String {
    format!("{ENTRIES_PATH}/{id}")
}

/// User agent a fetch sends when the feed names none.
pub const DEFAULT_USER_AGENT: &str = concat!("lockbox/", env!("CARGO_PKG_VERSION"));

/// Sort which the feed list's cursors are made for.
pub const FEEDS_SORT: &str = "reader/feeds";

/// Sort which the reading list's cursors are made for.
pub const ENTRIES_SORT: &str = "reader/entries";

/// Filter and paging of the reading list route.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EntryQuery {
    /// Only show entries of this feed.
    pub feed: Option<i64>,
    /// Only show read entries when true.
    pub read: Option<bool>,
    /// Only show starred entries when true.
    pub starred: Option<bool>,
    /// Only archived entries when true.
    pub archived: Option<bool>,
    /// Words to match against title, summary and body.
    pub query: Option<String>,
    /// How many items the page holds.
    pub limit: Option<usize>,
    /// Cursor which the previous page answered with.
    pub cursor: Option<String>,
}

impl EntryQuery {
    /// This query as a request string, which starts with `?` when it holds
    /// anything at all.
    pub fn to_query(&self) -> String {
        query_string(&[
            ("feed", self.feed.map(|feed| feed.to_string())),
            ("read", self.read.map(|read| read.to_string())),
            ("starred", self.starred.map(|starred| starred.to_string())),
            (
                "archived",
                self.archived.map(|archived| archived.to_string()),
            ),
            ("query", self.query.clone()),
            ("limit", self.limit.map(|limit| limit.to_string())),
            ("cursor", self.cursor.clone()),
        ])
    }

    /// The filter which this query asks the store for.
    pub fn filter(&self) -> Filter {
        Filter {
            feed_id: self.feed,
            read: self.read,
            starred: self.starred,
            archived: self.archived,
            query: self.query.clone(),
        }
    }
}
