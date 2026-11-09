//! The local database, which holds everything made by the user.
//!
//! [`open`] applies its migrations and each domain module holds the queries
//! against one group of tables.

use std::{io, path::Path};

use rusqlite::{
    Connection, params,
    types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
};

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
];

/// Open the local database at `path`.
pub fn open(path: &Path) -> Result<Db, Error> {
    Db::open(path, MIGRATIONS)
}

/// Whether a piece of data is published to the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    /// Published as a record.
    Public,
    /// Kept on this node.
    Private,
}

impl Visibility {
    /// Name used in the database.
    fn text(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Private => "private",
        }
    }
}

impl ToSql for Visibility {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.text().into())
    }
}

impl FromSql for Visibility {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "public" => Ok(Self::Public),
            "private" => Ok(Self::Private),
            other => Err(unknown("visibility", other)),
        }
    }
}

/// Error for a column value the code does not know.
pub(super) fn unknown(column: &str, value: &str) -> FromSqlError {
    FromSqlError::Other(io::Error::other(format!("unknown {column} {value}")).into())
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
