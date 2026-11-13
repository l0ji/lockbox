//! The local database, which holds everything made by the user.
//!
//! [`open`] applies its migrations and each domain module holds the queries
//! against one group of tables.

use std::{io, path::Path};

use rusqlite::{Connection, Row, params, types::FromSqlError};

use crate::{Db, Error};

pub mod bookmarks;
pub mod entries;
pub mod feeds;
pub mod folders;
pub mod settings;
pub mod tags;
pub mod topics;

/// SQL scripts applied to a local database, in order.
pub const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/local/0001_reader.sql"),
    include_str!("../../migrations/local/0002_entry_search.sql"),
    include_str!("../../migrations/local/0003_bookmarks.sql"),
    include_str!("../../migrations/local/0004_bookmark_search.sql"),
    include_str!("../../migrations/local/0005_preferences.sql"),
    include_str!("../../migrations/local/0006_feed_fetch.sql"),
];

/// Open the local database at `path`.
pub fn open(path: &Path) -> Result<Db, Error> {
    Db::open(path, MIGRATIONS)
}

/// One page of a list query, with the position the next page starts after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page<T, C> {
    /// Rows in this page.
    pub items: Vec<T>,
    /// Where the next page starts, absent at the end.
    pub next: Option<C>,
}

/// Error for a column value the code does not know.
pub(super) fn unknown(column: &str, value: &str) -> FromSqlError {
    FromSqlError::Other(io::Error::other(format!("unknown {column} {value}")).into())
}

/// Quote search terms for FTS5, keeping AND, OR and NOT as operators between words.
pub(super) fn search_query(query: &str) -> Option<String> {
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }
    let terms: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(position, word)| {
            let operator = position > 0
                && position + 1 < words.len()
                && matches!(word.to_ascii_uppercase().as_str(), "OR" | "AND" | "NOT");
            match operator {
                true => word.to_ascii_uppercase(),
                false => format!("\"{}\"", word.replace('"', "\"\"")),
            }
        })
        .collect();
    Some(terms.join(" "))
}

/// Read a column which holds the name of an enum, and parse it.
pub(super) fn named<T>(
    row: &Row<'_>,
    index: usize,
    column: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    let text: String = row.get(index)?;
    Ok(parse(&text).ok_or_else(|| unknown(column, &text))?)
}

/// A table whose rows are ordered by `position` inside a parent.
#[derive(Debug, Clone, Copy)]
pub(super) enum Placed {
    /// Bookmarks inside a folder.
    Bookmarks,
    /// Folders inside their parent.
    Folders,
}

impl Placed {
    /// Table that holds the rows.
    fn table(self) -> &'static str {
        match self {
            Self::Bookmarks => "bookmark",
            Self::Folders => "folder",
        }
    }

    /// Column that names the parent row.
    fn parent(self) -> &'static str {
        match self {
            Self::Bookmarks => "folder_id",
            Self::Folders => "parent_id",
        }
    }
}

/// Put `moved` at `position` among its siblings and number the group from zero.
///
/// `parent` is a nullable foreign key, so the comparison has to use `IS`.
pub(super) fn renumber(
    connection: &Connection,
    placed: Placed,
    parent: Option<i64>,
    moved: i64,
    position: usize,
) -> Result<(), Error> {
    let table = placed.table();
    let parent_column = placed.parent();

    let mut siblings: Vec<i64> = {
        let mut statement = connection.prepare(&format!(
            "SELECT id FROM {table} WHERE {parent_column} IS ?1 AND id <> ?2
             ORDER BY position, id"
        ))?;
        let rows = statement.query_map(params![parent, moved], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    siblings.insert(position.min(siblings.len()), moved);

    let mut statement =
        connection.prepare(&format!("UPDATE {table} SET position = ?1 WHERE id = ?2"))?;
    for (position, id) in siblings.iter().enumerate() {
        statement.execute(params![position as i64, id])?;
    }
    Ok(())
}
