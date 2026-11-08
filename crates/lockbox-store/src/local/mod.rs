//! The local database, which holds everything made by the user.
//!
//! [`open`] applies its migrations and each domain module holds the queries
//! against one group of tables.

use std::{io, path::Path};

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::{Db, Error};

pub mod entries;
pub mod feeds;

/// SQL scripts applied to a local database, in order.
pub const MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/local/0001_reader.sql"),
    include_str!("../../migrations/local/0002_entry_search.sql"),
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
