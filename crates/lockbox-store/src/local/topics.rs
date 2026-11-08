//! This module stores the topics namespace, which define what a link is about.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Db, Error};

/// Columns that every topic query returns, in the order [`from_row`] reads
/// them.
pub(super) const COLUMNS: &str = "id, name, created_at, followed_at";

/// One topic in the reader's namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    /// When the user started following the topic, if they do.
    pub followed_at: Option<i64>,
}

/// Every topic, by name.
pub async fn list(db: &Db) -> Result<Vec<Topic>, Error> {
    db.call(|connection| {
        let sql = format!("SELECT {COLUMNS} FROM topic ORDER BY name");
        let mut statement = connection.prepare(&sql)?;
        let topics = statement.query_map([], from_row)?;
        Ok(topics.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

/// Rename one topic.
pub async fn rename(db: &Db, id: i64, name: &str) -> Result<Topic, Error> {
    let name = name.to_owned();
    db.call(move |connection| {
        let sql = format!("UPDATE topic SET name = ?1 WHERE id = ?2 RETURNING {COLUMNS}");
        match connection
            .query_row(&sql, params![name, id], from_row)
            .optional()?
        {
            Some(topic) => Ok(topic),
            None => Err(Error::NotFound),
        }
    })
    .await
}

/// Remove one topic, which also removes it from all bookmarks that have it
/// assigned to them.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute("DELETE FROM topic WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// The topics one bookmark carries, ordered by name.
pub(super) fn of_bookmark(connection: &Connection, bookmark_id: i64) -> Result<Vec<Topic>, Error> {
    let mut statement = connection.prepare(
        "SELECT t.id, t.name, t.created_at, t.followed_at
         FROM topic t JOIN bookmark_topic bt ON bt.topic_id = t.id
         WHERE bt.bookmark_id = ?1
         ORDER BY t.name",
    )?;
    let topics = statement.query_map([bookmark_id], from_row)?;
    Ok(topics.collect::<Result<Vec<_>, _>>()?)
}

/// Resolve `names` to topics, creating the ones that do not exist yet.
///
/// Names are trimmed, matched with insensitive casing and deduplicated, and
/// blank ones are dropped.
pub(super) fn resolve(
    connection: &Connection,
    names: &[String],
    now: i64,
) -> Result<Vec<Topic>, Error> {
    let mut topics: Vec<Topic> = Vec::new();
    for name in names {
        let name = name.trim();
        if name.is_empty()
            || topics
                .iter()
                .any(|topic| topic.name.eq_ignore_ascii_case(name))
        {
            continue;
        }
        topics.push(find_or_create(connection, name, now)?);
    }
    Ok(topics)
}

/// Find a topic called `name`, which is automatically created when it does not
/// exist yet.
pub(super) fn find_or_create(
    connection: &Connection,
    name: &str,
    now: i64,
) -> Result<Topic, Error> {
    let sql = format!("SELECT {COLUMNS} FROM topic WHERE name = ?1");
    match connection.query_row(&sql, [name], from_row).optional()? {
        Some(topic) => Ok(topic),
        None => Ok(connection.query_row(
            &format!("INSERT INTO topic (name, created_at) VALUES (?1, ?2) RETURNING {COLUMNS}"),
            params![name, now],
            from_row,
        )?),
    }
}

/// Read one topic row.
pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Topic> {
    Ok(Topic {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        followed_at: row.get(3)?,
    })
}
