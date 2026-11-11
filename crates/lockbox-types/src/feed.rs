//! Feeds the reader follows, the state of their last refresh and the changes
//! the user can make to them.

use serde::{Deserialize, Serialize};

use crate::entry::NewEntry;

/// Seconds a fetch waits for the server when the user names no timeout.
pub const DEFAULT_TIMEOUT_SECONDS: i64 = 20;

/// Seconds between refreshes when the user names no interval.
pub const DEFAULT_REFRESH_SECONDS: i64 = 3600;

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

impl Kind {
    /// Name the format is written with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Atom => "atom",
            Self::Json => "json",
        }
    }

    /// Read a name which [`Kind::as_str`] wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "rss" => Some(Self::Rss),
            "atom" => Some(Self::Atom),
            "json" => Some(Self::Json),
            _ => None,
        }
    }
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

impl Visibility {
    /// Name the visibility is written with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }

    /// Read a name which [`Visibility::as_str`] wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "public" => Some(Self::Public),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
}

/// Where a feed was found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Source {
    /// Added by the user.
    #[default]
    User,
    /// Advertised by a site the reader looked at.
    Page,
    /// Named by another account's records.
    Federation,
}

impl Source {
    /// Name the provenance is written with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Page => "page",
            Self::Federation => "federation",
        }
    }

    /// Read a name which [`Source::as_str`] wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "user" => Some(Self::User),
            "page" => Some(Self::Page),
            "federation" => Some(Self::Federation),
            _ => None,
        }
    }
}

/// One feed the reader follows.
///
/// The validators and the provenance are the node's own state, so they are
/// left out of what it writes to a client.
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
    /// User agent a fetch sends, or the node's own when it is missing.
    pub user_agent: Option<String>,
    /// When the next refresh is due.
    pub next_refresh_at: Option<i64>,
    /// When the feed was last fetched.
    pub fetched_at: Option<i64>,
    /// Validator to send on the next fetch.
    #[serde(skip)]
    pub etag: Option<String>,
    /// Date to send on the next fetch.
    #[serde(skip)]
    pub last_modified: Option<String>,
    /// Why the last refresh failed.
    pub last_error: Option<String>,
    /// Whether the feed is published.
    pub visibility: Visibility,
    /// Whether refreshing is paused.
    pub disabled: bool,
    /// How the feed was found.
    #[serde(skip)]
    pub discovered_from: Source,
    /// When the feed was added.
    pub created_at: i64,
    /// When the feed was last changed.
    pub updated_at: i64,
}

impl Feed {
    /// Name the feed list orders by, which is the title or the URL.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.url)
    }
}

/// A feed to add to the reader.
///
/// The address, the title and the fetch settings are what the user asked for.
/// The site, the format and the provenance are the node's to fill in, so they
/// never cross the wire.
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
    /// User agent the first fetch sends, or the node's own when it is missing.
    pub user_agent: Option<String>,
    /// Site the feed belongs to, when the document names one.
    #[serde(skip)]
    pub site_url: Option<String>,
    /// Format the document was written in.
    #[serde(skip)]
    pub kind: Option<Kind>,
    /// How the feed was found.
    #[serde(skip)]
    pub discovered_from: Source,
}

impl NewFeed {
    /// A feed the user added themselves, with the node's defaults.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            title: None,
            refresh_seconds: None,
            timeout_seconds: None,
            user_agent: None,
            site_url: None,
            kind: None,
            discovered_from: Source::User,
        }
    }
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
    #[serde(default, deserialize_with = "crate::nullable")]
    pub user_agent: Option<Option<String>>,
    /// Whether the feed is published.
    pub visibility: Option<Visibility>,
    /// Whether refreshing is paused.
    pub disabled: Option<bool>,
}

/// What one successful fetch of a feed found.
#[derive(Debug, Clone)]
pub struct Refresh {
    /// When the feed was fetched.
    pub fetched_at: i64,
    /// Validator to send on the next fetch.
    pub etag: Option<String>,
    /// Date to send on the next fetch.
    pub last_modified: Option<String>,
    /// Entries the fetch parsed.
    pub entries: Vec<NewEntry>,
}

/// What one refresh of a feed found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Refreshed {
    /// The feed after the refresh.
    pub feed: Feed,
    /// Entries which the refresh added.
    pub new_entries: usize,
}
