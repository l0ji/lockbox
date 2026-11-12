//! The topics namespace, which says what a link is about.

use serde::{Deserialize, Serialize};

/// One topic in the reader's namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Topic {
    /// Row of the topic.
    pub id: i64,
    /// Name it is written with.
    pub name: String,
    /// When it was first used.
    pub created_at: i64,
    /// When the user started following it, if they do.
    pub followed_at: Option<i64>,
}

/// Settings of a topic which the user can change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopicChange {
    /// Name to give the topic.
    pub name: Option<String>,
}
