//! Routes of the reader view: the feeds the user follows and the entries
//! they read.

use serde::{Deserialize, Serialize};

use super::{nullable, query_string};

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

/// Format of a feed document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Any version of RSS.
    Rss,
    /// Atom.
    Atom,
    /// JSON Feed.
    Json,
}

/// Whether a feed is published to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Published as records.
    Public,
    /// Kept on this node.
    Private,
}

/// One feed the reader follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    /// Row of the feed.
    pub id: i64,
    /// Address of the document.
    pub url: String,
    /// Site the feed belongs to.
    pub site_url: Option<String>,
    /// Title to show in the reader.
    pub title: Option<String>,
    /// Format of the document.
    pub kind: Kind,
    /// Seconds between refreshes.
    pub refresh_seconds: i64,
    /// Seconds the server has to answer a fetch.
    pub timeout_seconds: i64,
    /// User agent a fetch sends, or [`DEFAULT_USER_AGENT`] when it is missing.
    pub user_agent: Option<String>,
    /// When the next refresh is due.
    pub next_refresh_at: Option<i64>,
    /// When the feed was last fetched.
    pub fetched_at: Option<i64>,
    /// Why the last refresh failed.
    pub last_error: Option<String>,
    /// Whether the feed is published.
    pub visibility: Visibility,
    /// Whether refreshing is paused.
    pub disabled: bool,
    /// When the feed was added.
    pub created_at: i64,
    /// When the feed was last changed.
    pub updated_at: i64,
}

/// Feed which the user asked to add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewFeed {
    /// Address the user gave, which may be the feed or a page with one.
    pub url: String,
    /// Title to show instead of the document's own.
    pub title: Option<String>,
    /// Seconds between refreshes.
    pub refresh_seconds: Option<i64>,
    /// Seconds the server has to answer a fetch, which the node defaults.
    pub timeout_seconds: Option<i64>,
    /// User agent the first fetch sends, or [`DEFAULT_USER_AGENT`] when it is missing.
    pub user_agent: Option<String>,
}

/// Settings of a feed which the user can change.
///
/// A field which is left out keeps its current value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedChange {
    /// Address to fetch.
    pub url: Option<String>,
    /// Site the feed belongs to.
    pub site_url: Option<String>,
    /// Title to show in the reader.
    pub title: Option<String>,
    /// Format to parse the document as.
    pub kind: Option<Kind>,
    /// Seconds between refreshes.
    pub refresh_seconds: Option<i64>,
    /// Seconds the server has to answer a fetch.
    pub timeout_seconds: Option<i64>,
    /// User agent a fetch sends, where a `null` returns to the default.
    #[serde(default, deserialize_with = "nullable")]
    pub user_agent: Option<Option<String>>,
    /// Whether the feed is published.
    pub visibility: Option<Visibility>,
    /// Whether refreshing is paused.
    pub disabled: Option<bool>,
}

/// What one refresh of a feed found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refreshed {
    /// The feed after the refresh.
    pub feed: Feed,
    /// Entries which the refresh added.
    pub new_entries: usize,
}

/// What a reader list shows for one entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntrySummary {
    /// Row of the entry.
    pub id: i64,
    /// Identifier the store gave the entry.
    pub guid: String,
    /// Feed which brought the entry.
    pub feed_id: i64,
    /// Title of the entry.
    pub title: String,
    /// Page the entry links to.
    pub url: Option<String>,
    /// Who wrote it.
    pub author: Option<String>,
    /// Short summary the feed gave.
    pub summary: Option<String>,
    /// When the feed says it was published.
    pub published_at: Option<i64>,
    /// Ordering key of the entry.
    pub sort_at: i64,
    /// When the entry was fetched.
    pub fetched_at: i64,
    /// When the entry was read.
    pub read_at: Option<i64>,
    /// When the entry was starred.
    pub starred_at: Option<i64>,
    /// When the entry was archived.
    pub archived_at: Option<i64>,
}

/// One entry with everything the reader shows of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Everything a list shows.
    #[serde(flatten)]
    pub summary: EntrySummary,
    /// Body as the feed gave it.
    pub body: Option<String>,
    /// Whether the feed had already cut the body.
    pub truncated: bool,
}

/// Reader state which a change turns on or off.
///
/// A field which is left out keeps its current value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryChange {
    /// Whether the entry is read.
    pub read: Option<bool>,
    /// Whether the entry is starred.
    pub starred: Option<bool>,
    /// Whether the entry is archived.
    pub archived: Option<bool>,
}

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
}
