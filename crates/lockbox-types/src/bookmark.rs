//! Links which are saved into the reader.

use serde::{Deserialize, Serialize};

use crate::{topic::Topic, visibility::Visibility};

/// One saved link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    /// Row of the bookmark.
    pub id: i64,
    /// Address the user saved.
    pub url: String,
    /// Address that decides whether the link is already saved.
    pub canonical_url: String,
    /// Title to show in the reader.
    pub title: String,
    /// Note the user wrote about it.
    pub note: Option<String>,
    /// Folder it is filed into, if any.
    pub folder_id: Option<i64>,
    /// Place inside its folder.
    pub position: i64,
    /// Entry it was saved from, if any.
    pub entry_id: Option<i64>,
    /// When it was saved.
    pub created_at: i64,
    /// When it was last changed.
    pub updated_at: i64,
    /// When it was archived.
    pub archived_at: Option<i64>,
    /// Whether it is published.
    pub visibility: Visibility,
    /// Topics it carries, by name.
    pub topics: Vec<Topic>,
}

/// A bookmark to save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBookmark {
    /// Address the user saved.
    pub url: String,
    /// Address that decides whether the link is already saved.
    pub canonical_url: String,
    /// Title to show in the reader.
    pub title: String,
    /// Note the user wrote about it.
    pub note: Option<String>,
    /// Folder it is filed into, if any.
    pub folder_id: Option<i64>,
    /// Entry it was saved from, if any.
    pub entry_id: Option<i64>,
    /// Topics to put on it.
    pub topics: Vec<String>,
    /// Whether it is published.
    pub visibility: Visibility,
}

impl NewBookmark {
    /// A link saved under its own address with no topics.
    pub fn new(url: impl Into<String>, title: impl Into<String>, visibility: Visibility) -> Self {
        let url = url.into();
        Self {
            canonical_url: url.clone(),
            url,
            title: title.into(),
            note: None,
            folder_id: None,
            entry_id: None,
            topics: Vec::new(),
            visibility,
        }
    }
}

/// Which bookmarks a list looks at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Every bookmark.
    #[default]
    All,
    /// Bookmarks that are not in a folder.
    Unfiled,
    /// Bookmarks filed in or matched by one folder.
    Folder(i64),
}

/// Which bookmarks a list returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Folder scope of the list.
    pub folder: Scope,
    /// Only bookmarks which are carrying this topic.
    pub topic: Option<String>,
    /// Show only archived bookmarks when true, only live ones when false.
    pub archived: Option<bool>,
    /// Words to match against the title, note and URL.
    pub query: Option<String>,
}

/// Bookmark settings the user can change. A field which is left out keeps its
/// current value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BookmarkChange {
    /// Address to save instead.
    pub url: Option<String>,
    /// Address which decides identity, filled by the node and never by a client.
    #[serde(skip)]
    pub canonical_url: Option<String>,
    /// Title to show in the reader.
    pub title: Option<String>,
    /// Note to write, where a `null` clears it.
    #[serde(default, deserialize_with = "crate::nullable")]
    pub note: Option<Option<String>>,
    /// Folder to file it into, where a `null` unfiles it.
    #[serde(default, deserialize_with = "crate::nullable")]
    pub folder_id: Option<Option<i64>>,
    /// Place inside the folder.
    pub position: Option<usize>,
    /// Whether it is archived.
    pub archived: Option<bool>,
    /// Whether it is published.
    pub visibility: Option<Visibility>,
}
