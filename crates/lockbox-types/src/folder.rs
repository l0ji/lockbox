//! The folders which bookmarks are filed into or matched by.

use serde::{Deserialize, Serialize};

use crate::topic::Topic;

/// What decides a folder's contents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Bookmarks are manually added to this folder.
    #[default]
    Manual,
    /// Folder contents are dynamically generated based on given topics.
    Dynamic,
}

impl Kind {
    /// Name the kind is written with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Dynamic => "dynamic",
        }
    }

    /// Read a name which [`Kind::as_str`] wrote.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "manual" => Some(Self::Manual),
            "dynamic" => Some(Self::Dynamic),
            _ => None,
        }
    }
}

/// One folder in the reader's tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// Row of the folder.
    pub id: i64,
    /// Folder it sits in, or the root when it is missing.
    pub parent_id: Option<i64>,
    /// Name to show in the reader.
    pub name: String,
    /// What decides its contents.
    pub kind: Kind,
    /// Place among its siblings.
    pub position: i64,
    /// When it was created.
    pub created_at: i64,
    /// Topics a dynamic folder matches, by name.
    pub topics: Vec<Topic>,
}

/// A folder to create.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewFolder {
    /// Folder to place it in, or the root when it is missing.
    pub parent_id: Option<i64>,
    /// Name to show in the reader.
    pub name: String,
    /// What decides its contents.
    pub kind: Kind,
    /// Topics a dynamic folder matches.
    pub topics: Vec<String>,
}

impl NewFolder {
    /// An empty manual folder.
    pub fn new(parent_id: Option<i64>, name: impl Into<String>) -> Self {
        Self {
            parent_id,
            name: name.into(),
            kind: Kind::Manual,
            topics: Vec::new(),
        }
    }
}

/// Folder settings the user can change. A field which is left out keeps its
/// current value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderChange {
    /// Name to show in the reader.
    pub name: Option<String>,
    /// Folder to move it into, where a `null` puts it at the root.
    #[serde(default, deserialize_with = "crate::nullable")]
    pub parent_id: Option<Option<i64>>,
    /// Place among its siblings.
    pub position: Option<usize>,
    /// Topics a dynamic folder matches, which replaces the set it has.
    pub topics: Option<Vec<String>>,
}
