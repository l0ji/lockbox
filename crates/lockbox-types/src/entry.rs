//! Entries a feed brought, and the reader's view of them.

use serde::{Deserialize, Serialize};

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
    /// Ordering key: the published date, or the fetch time when there is none.
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

/// An entry as a refresh found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    /// Identifier the feed gives the item, or its URL when it has none.
    pub source: String,
    /// Page the item links to.
    pub url: Option<String>,
    /// Address that decides whether the item is already known.
    pub canonical_url: Option<String>,
    /// Title of the item, which falls back to its address.
    pub title: String,
    /// Who wrote it.
    pub author: Option<String>,
    /// Short summary the feed gave.
    pub summary: Option<String>,
    /// Body as the feed gave it, kept whole.
    pub body: Option<String>,
    /// Whether the source itself had already cut the body.
    pub truncated: bool,
    /// When the feed says it was published.
    pub published_at: Option<i64>,
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

/// Which entries a list returns. An empty filter returns every entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only entries from this feed.
    pub feed_id: Option<i64>,
    /// Only read entries when true, only unread ones when false.
    pub read: Option<bool>,
    /// Only starred entries when true, only unstarred ones when false.
    pub starred: Option<bool>,
    /// Only archived entries when true, only unarchived ones when false.
    pub archived: Option<bool>,
    /// Words to match against title, summary and body.
    pub query: Option<String>,
}
