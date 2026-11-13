//! Topics assigned to bookmarks and followed by the user.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use lockbox_types::topic::Topic;

use crate::{Db, Error};

/// Columns that every topic query returns, in the order [`from_row`] reads
/// them.
const COLUMNS: &str = "id, name, created_at, followed_at";

/// Position in the topic list from which the next page starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// Name of the last topic on the page.
    pub name: String,
    pub id: i64,
}

/// One page of topics.
pub type Page = super::Page<Topic, Cursor>;

/// Every topic, by name.
pub async fn list(db: &Db) -> Result<Vec<Topic>, Error> {
    db.call(|connection| {
        let sql = format!("SELECT {COLUMNS} FROM topic ORDER BY name COLLATE NOCASE, id");
        let mut statement = connection.prepare(&sql)?;
        let topics = statement.query_map([], from_row)?;
        Ok(topics.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

/// List topics by name, with the cursor saying where the last page ended.
pub async fn page(db: &Db, after: Option<Cursor>, limit: usize) -> Result<Page, Error> {
    db.call(move |connection| {
        if limit == 0 {
            return Ok(Page {
                items: Vec::new(),
                next: None,
            });
        }
        let wanted = limit.min(i64::MAX as usize) as i64;

        let mut sql = format!("SELECT {COLUMNS} FROM topic");
        let mut args: Vec<Value> = Vec::new();
        if let Some(cursor) = after {
            sql.push_str(
                " WHERE (name COLLATE NOCASE > ?1
                   OR (name COLLATE NOCASE = ?1 AND id > ?2))",
            );
            args.push(Value::Text(cursor.name));
            args.push(Value::Integer(cursor.id));
        }
        sql.push_str(" ORDER BY name COLLATE NOCASE, id LIMIT ?");
        args.push(Value::Integer(wanted.saturating_add(1)));

        let mut statement = connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(args))?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            items.push(from_row(row)?);
        }

        let next = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|last| Cursor {
                name: last.name.clone(),
                id: last.id,
            })
        } else {
            None
        };
        Ok(Page { items, next })
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> Result<Option<Topic>, Error> {
    db.call(move |connection| {
        let sql = format!("SELECT {COLUMNS} FROM topic WHERE id = ?1");
        Ok(connection.query_row(&sql, [id], from_row).optional()?)
    })
    .await
}

pub async fn rename(db: &Db, id: i64, name: &str) -> Result<Topic, Error> {
    let name = name.trim().to_owned();
    db.call(move |connection| {
        if name.is_empty() {
            return Err(Error::Invalid("a topic needs a name".to_owned()));
        }
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

/// Follow a topic, creating it if needed and preserving any existing
/// timestamps.
pub async fn follow(db: &Db, name: &str, now: i64) -> Result<Topic, Error> {
    let name = name.trim().to_owned();
    db.transaction(move |transaction| {
        if name.is_empty() {
            return Err(Error::Invalid("a topic needs a name".to_owned()));
        }
        let topic = find_or_create(transaction, &name, now)?;
        let sql = format!(
            "UPDATE topic SET followed_at = coalesce(followed_at, ?1)
             WHERE id = ?2 RETURNING {COLUMNS}"
        );
        let topic = transaction.query_row(&sql, params![now, topic.id], from_row)?;
        Ok(topic)
    })
    .await
}

/// Stop following a topic. Does nothing if the topic was never followed.
pub async fn unfollow(db: &Db, name: &str) -> Result<(), Error> {
    let name = name.trim().to_owned();
    db.call(move |connection| {
        connection.execute(
            "UPDATE topic SET followed_at = NULL WHERE name = ?1 AND followed_at IS NOT NULL",
            [&name],
        )?;
        Ok(())
    })
    .await
}

/// The topics the user follows, sorted by name.
pub async fn followed(db: &Db) -> Result<Vec<Topic>, Error> {
    db.call(|connection| {
        let sql =
            format!("SELECT {COLUMNS} FROM topic WHERE followed_at IS NOT NULL ORDER BY name");
        let mut statement = connection.prepare(&sql)?;
        let topics = statement.query_map([], from_row)?;
        Ok(topics.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

/// Delete a topic and remove it from bookmarks and folders.
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
    let mut topics = of_bookmarks(connection, &[bookmark_id])?;
    Ok(topics.remove(&bookmark_id).unwrap_or_default())
}

/// Topics keyed by bookmark ID, ordered by name.
pub(super) fn of_bookmarks(
    connection: &Connection,
    ids: &[i64],
) -> Result<HashMap<i64, Vec<Topic>>, Error> {
    let mut topics: HashMap<i64, Vec<Topic>> = HashMap::new();
    if ids.is_empty() {
        return Ok(topics);
    }

    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut statement = connection.prepare(&format!(
        "SELECT t.id, t.name, t.created_at, t.followed_at, bt.bookmark_id
         FROM bookmark_topic bt JOIN topic t ON t.id = bt.topic_id
         WHERE bt.bookmark_id IN ({placeholders})
         ORDER BY bt.bookmark_id, t.name"
    ))?;
    let rows = statement.query_map(params_from_iter(ids.iter().copied()), |row| {
        Ok((row.get::<_, i64>(4)?, from_row(row)?))
    })?;
    for row in rows {
        let (bookmark, topic) = row?;
        topics.entry(bookmark).or_default().push(topic);
    }
    Ok(topics)
}

/// Find or create topics, trimming names and skipping blanks and case-insensitive duplicates.
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

pub(super) fn from_row(row: &Row<'_>) -> rusqlite::Result<Topic> {
    Ok(Topic {
        id: row.get(0)?,
        name: row.get(1)?,
        created_at: row.get(2)?,
        followed_at: row.get(3)?,
    })
}
