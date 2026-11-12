//! Routes of the reader view: the feeds the user follows, the entries they
//! read and the bookmarks they save.

use lockbox_types::{
    bookmark::{Filter, Scope},
    entry,
    visibility::Visibility,
};
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

/// Route which saves one entry as a bookmark.
pub fn save_entry_path(id: i64) -> String {
    format!("{ENTRIES_PATH}/{id}/save")
}

/// Route which lists every bookmark and saves one.
pub const BOOKMARKS_PATH: &str = "/api/reader/bookmarks";

/// Route which reads, changes and removes one bookmark.
pub fn bookmark_path(id: i64) -> String {
    format!("{BOOKMARKS_PATH}/{id}")
}

/// Route which replaces the topics one bookmark carries.
pub fn bookmark_topics_path(id: i64) -> String {
    format!("{BOOKMARKS_PATH}/{id}/topics")
}

/// Route which lists every folder and creates one.
pub const FOLDERS_PATH: &str = "/api/reader/folders";

/// Route which changes or removes one folder.
pub fn folder_path(id: i64) -> String {
    format!("{FOLDERS_PATH}/{id}")
}

/// Route which lists every topic.
pub const TOPICS_PATH: &str = "/api/reader/topics";

/// Route which renames or removes one topic.
pub fn topic_path(id: i64) -> String {
    format!("{TOPICS_PATH}/{id}")
}

/// User agent a fetch sends when the feed names none.
pub const DEFAULT_USER_AGENT: &str = concat!("lockbox/", env!("CARGO_PKG_VERSION"));

/// Sort which the feed list's cursors are made for.
pub const FEEDS_SORT: &str = "reader/feeds";

/// Sort which the reading list's cursors are made for.
pub const ENTRIES_SORT: &str = "reader/entries";

/// Sort which the bookmark list's cursors are made for when it is newest first.
pub const BOOKMARKS_RECENT_SORT: &str = "reader/bookmarks/recent";

/// Sort which the bookmark list's cursors are made for inside a manual folder.
pub const BOOKMARKS_ORDER_SORT: &str = "reader/bookmarks/order";

/// Sort which the topic list's cursors are made for.
pub const TOPICS_SORT: &str = "reader/topics";

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
    pub fn filter(&self) -> entry::Filter {
        entry::Filter {
            feed_id: self.feed,
            read: self.read,
            starred: self.starred,
            archived: self.archived,
            query: self.query.clone(),
        }
    }
}

/// Filter and paging of the bookmark list route.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BookmarkQuery {
    /// Folder to list: its row, `unfiled`, or every folder when it is missing.
    pub folder: Option<String>,
    /// Only bookmarks which are carrying this topic.
    pub topic: Option<String>,
    /// List archived bookmarks when set to true.
    pub archived: Option<bool>,
    /// Text search against the title, note and address.
    pub query: Option<String>,
    pub limit: Option<usize>,
    pub cursor: Option<String>,
}

impl BookmarkQuery {
    /// This query as a request string, which starts with `?` when it holds
    /// anything at all.
    pub fn to_query(&self) -> String {
        query_string(&[
            ("folder", self.folder.clone()),
            ("topic", self.topic.clone()),
            (
                "archived",
                self.archived.map(|archived| archived.to_string()),
            ),
            ("query", self.query.clone()),
            ("limit", self.limit.map(|limit| limit.to_string())),
            ("cursor", self.cursor.clone()),
        ])
    }

    /// The folder scope which the query names.
    pub fn scope(&self) -> Result<Scope, super::ApiError> {
        match self.folder.as_deref() {
            None | Some("") | Some("all") => Ok(Scope::All),
            Some("unfiled") => Ok(Scope::Unfiled),
            Some(folder) => folder.parse().map(Scope::Folder).map_err(|_| {
                super::ApiError::invalid(format!("folder {folder} is not a row or unfiled"))
            }),
        }
    }

    /// The filter which this query asks the store for.
    pub fn filter(&self) -> Result<Filter, super::ApiError> {
        Ok(Filter {
            folder: self.scope()?,
            topic: self.topic.clone(),
            archived: Some(self.archived.unwrap_or(false)),
            query: self.query.clone(),
        })
    }
}

/// A bookmark which the user asked the node to save.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookmarkRequest {
    pub url: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub visibility: Option<Visibility>,
}

/// The topics which a bookmark should carry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TopicsRequest {
    pub topics: Vec<String>,
}

/// What to do with an entry which is being saved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SaveEntry {
    pub folder_id: Option<i64>,
    pub topics: Vec<String>,
    pub note: Option<String>,
    pub visibility: Option<Visibility>,
}
