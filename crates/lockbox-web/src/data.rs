//! Shared page data and form values.

use std::collections::HashMap;

use lockbox_proto::api::{
    ListQuery, Page,
    node::SETTINGS_PATH,
    reader::{BookmarkQuery, FOLDERS_PATH, TOPICS_PATH},
};
use lockbox_types::{folder::Folder, topic::Topic};
use serde::Deserialize;

use crate::{ClientError, DaemonClient};

/// Every folder, as one flat list ordered by parent and position.
pub(crate) async fn folders(client: &DaemonClient) -> Result<Vec<Folder>, ClientError> {
    client.get_json(FOLDERS_PATH).await
}

/// The first `limit` topics, by name.
pub(crate) async fn topics(client: &DaemonClient, limit: usize) -> Result<Vec<Topic>, ClientError> {
    let query = ListQuery {
        limit: Some(limit),
        cursor: None,
    };
    let page: Page<Topic> = client
        .get_json(&format!("{TOPICS_PATH}{}", query.to_query()))
        .await?;
    Ok(page.items)
}

pub(crate) async fn publish_bookmarks(client: &DaemonClient) -> Result<bool, ClientError> {
    #[derive(Deserialize)]
    struct Settings {
        publish_bookmarks: bool,
    }

    let settings: Settings = client.get_json(SETTINGS_PATH).await?;
    Ok(settings.publish_bookmarks)
}

/// One folder with its place in the tree.
pub(crate) struct TreeFolder {
    pub depth: usize,
    pub folder: Folder,
    pub last: bool,
}

/// Folders in tree order, with depths and last-sibling markers.
pub(crate) fn tree(folders: &[Folder]) -> Vec<TreeFolder> {
    let known: Vec<i64> = folders.iter().map(|folder| folder.id).collect();
    let mut children: HashMap<Option<i64>, Vec<Folder>> = HashMap::new();
    for folder in folders {
        let parent = match folder.parent_id {
            Some(parent) if known.contains(&parent) => Some(parent),
            _ => None,
        };
        children.entry(parent).or_default().push(folder.clone());
    }
    for siblings in children.values_mut() {
        siblings.sort_by_key(|folder| (folder.position, folder.id));
    }

    let mut rows = Vec::new();
    walk(&mut children, None, 0, &mut rows);

    // Show unvisited folders at the root
    let mut orphans: Vec<Folder> = children.into_values().flatten().collect();
    orphans.sort_by_key(|folder| (folder.position, folder.id));
    for folder in orphans {
        rows.push(TreeFolder {
            depth: 0,
            folder,
            last: true,
        });
    }
    rows
}

fn walk(
    children: &mut HashMap<Option<i64>, Vec<Folder>>,
    parent: Option<i64>,
    depth: usize,
    rows: &mut Vec<TreeFolder>,
) {
    let siblings = children.remove(&parent).unwrap_or_default();
    let count = siblings.len();
    for (position, folder) in siblings.into_iter().enumerate() {
        let id = folder.id;
        rows.push(TreeFolder {
            depth,
            folder,
            last: position + 1 == count,
        });
        walk(children, Some(id), depth + 1, rows);
    }
}

/// A folder option in a form.
pub struct Choice {
    pub id: i64,
    pub label: String,
    pub selected: bool,
}

/// Folder choices in tree order, optionally limited to manual folders.
pub(crate) fn choices(folders: &[Folder], manual_only: bool, selected: Option<i64>) -> Vec<Choice> {
    tree(folders)
        .into_iter()
        .filter(|row| !manual_only || row.folder.kind == lockbox_types::folder::Kind::Manual)
        .map(|row| Choice {
            id: row.folder.id,
            label: format!("{}{}", tree_prefix(row.depth, row.last), row.folder.name),
            selected: selected == Some(row.folder.id),
        })
        .collect()
}

pub(crate) fn tree_prefix(depth: usize, last: bool) -> String {
    match depth {
        0 => String::new(),
        depth => format!(
            "{}{}",
            "\u{a0}".repeat((depth - 1) * 2),
            match last {
                true => "└─ ",
                false => "├─ ",
            }
        ),
    }
}

/// A link in a sidebar.
pub struct NavLink {
    pub title: String,
    pub href: String,
    pub selected: bool,
}

pub fn folder_links(folders: &[Folder], selected: Option<&str>, page: &str) -> Vec<NavLink> {
    let mut links = vec![
        NavLink {
            title: "all saved".to_owned(),
            href: page.to_owned(),
            selected: selected.is_none(),
        },
        NavLink {
            title: "unfiled".to_owned(),
            href: format!("{page}?folder=unfiled"),
            selected: selected == Some("unfiled"),
        },
    ];
    links.extend(tree(folders).into_iter().map(|row| {
        let folder = row.folder;
        let name = folder.id.to_string();
        NavLink {
            title: format!("{}{}", tree_prefix(row.depth, row.last), folder.name),
            href: format!("{page}?folder={}", folder.id),
            selected: selected == Some(name.as_str()),
        }
    }));
    links
}

pub fn topic_links(topics: &[Topic], selected: Option<&str>, page: &str) -> Vec<NavLink> {
    topics
        .iter()
        .map(|topic| NavLink {
            title: topic.name.clone(),
            href: format!(
                "{page}{}",
                BookmarkQuery {
                    topic: Some(topic.name.clone()),
                    ..BookmarkQuery::default()
                }
                .to_query()
            ),
            selected: selected == Some(topic.name.as_str()),
        })
        .collect()
}

/// The link host without the www prefix.
pub(crate) fn domain(url: &str) -> String {
    let rest = url.split("://").nth(1).unwrap_or(url);
    rest.split('/')
        .next()
        .unwrap_or(rest)
        .trim_start_matches("www.")
        .to_owned()
}

/// Splits comma-separated topics with whitespace checks.
pub(crate) fn topics_of(text: &str) -> Vec<String> {
    text.split(',')
        .map(str::trim)
        .filter(|topic| !topic.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Trims form text, treating blank input as absent.
pub(crate) fn optional_text(text: String) -> Option<String> {
    let text = text.trim();
    match text.is_empty() {
        true => None,
        false => Some(text.to_owned()),
    }
}

/// Parses a form row ID, treating invalid or missing input as absent.
pub(crate) fn id(value: &Option<String>) -> Option<i64> {
    value.as_deref().and_then(|value| value.parse().ok())
}
