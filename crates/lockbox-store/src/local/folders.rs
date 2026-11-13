//! The folders which bookmarks are locally filed into or matched by.

use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use lockbox_types::{
    folder::{Folder, FolderChange, Kind, NewFolder},
    topic::Topic,
};

use super::{Placed, named, renumber, topics};
use crate::{Db, Error};

/// Columns every folder query returns, in the order [`from_row`] reads them.
const COLUMNS: &str = "id, parent_id, name, kind, position, created_at";

/// Every folder, by parent and then by position.
pub async fn list(db: &Db) -> Result<Vec<Folder>, Error> {
    db.call(|connection| {
        let sql = format!("SELECT {COLUMNS} FROM folder ORDER BY parent_id, position, id");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map([], from_row)?;
        let mut folders = rows.collect::<Result<Vec<_>, _>>()?;

        let mut topics = all_topics(connection)?;
        for folder in &mut folders {
            folder.topics = topics.remove(&folder.id).unwrap_or_default();
        }
        Ok(folders)
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> Result<Option<Folder>, Error> {
    db.call(move |connection| get_in(connection, id)).await
}

/// Add a folder at the end of its siblings.
pub async fn create(db: &Db, folder: NewFolder, now: i64) -> Result<Folder, Error> {
    db.transaction(move |transaction| {
        let name = folder.name.trim().to_owned();
        if name.is_empty() {
            return Err(Error::Invalid("a folder needs a name".to_owned()));
        }
        if folder.kind == Kind::Manual && !folder.topics.is_empty() {
            return Err(Error::Invalid(
                "only a dynamic folder matches topics".to_owned(),
            ));
        }

        let position: i64 = transaction.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM folder WHERE parent_id IS ?1",
            params![folder.parent_id],
            |row| row.get(0),
        )?;
        let sql = format!(
            "INSERT INTO folder (parent_id, name, kind, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5) RETURNING {COLUMNS}"
        );
        let stored = transaction.query_row(
            &sql,
            params![folder.parent_id, name, folder.kind.as_str(), position, now],
            from_row,
        )?;

        if !folder.topics.is_empty() {
            replace_topics(transaction, stored.id, &folder.topics, now)?;
        }
        let stored = get_in(transaction, stored.id)?.ok_or(Error::NotFound)?;
        Ok(stored)
    })
    .await
}

pub async fn update(db: &Db, id: i64, change: FolderChange, now: i64) -> Result<Folder, Error> {
    db.transaction(move |transaction| {
        let current = get_in(transaction, id)?.ok_or(Error::NotFound)?;

        let name = match change.name {
            Some(name) => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return Err(Error::Invalid("a folder needs a name".to_owned()));
                }
                Some(name)
            }
            None => None,
        };
        if let Some(topics) = change.topics.as_ref()
            && current.kind != Kind::Dynamic
            && !topics.is_empty()
        {
            return Err(Error::Invalid(
                "only a dynamic folder matches topics".to_owned(),
            ));
        }

        let parent = change.parent_id.unwrap_or(current.parent_id);
        if let Some(parent) = parent
            && inside(transaction, id, parent)?
        {
            return Err(Error::Invalid(
                "a folder cannot move inside itself".to_owned(),
            ));
        }

        let mut sets: Vec<&str> = Vec::new();
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if let Some(name) = name {
            sets.push("name = ?");
            args.push(rusqlite::types::Value::Text(name));
        }
        if change.parent_id.is_some() {
            sets.push("parent_id = ?");
            args.push(match parent {
                Some(parent) => rusqlite::types::Value::Integer(parent),
                None => rusqlite::types::Value::Null,
            });
        }
        if !sets.is_empty() {
            args.push(rusqlite::types::Value::Integer(id));
            transaction.execute(
                &format!("UPDATE folder SET {} WHERE id = ?", sets.join(", ")),
                rusqlite::params_from_iter(args),
            )?;
        }

        if change.parent_id.is_some() || change.position.is_some() {
            let position = match change.position {
                Some(position) => position,
                None => {
                    let count: i64 = transaction.query_row(
                        "SELECT count(*) FROM folder WHERE parent_id IS ?1 AND id <> ?2",
                        params![parent, id],
                        |row| row.get(0),
                    )?;
                    count as usize
                }
            };
            renumber(transaction, Placed::Folders, parent, id, position)?;
        }

        if let Some(topics) = change.topics {
            replace_topics(transaction, id, &topics, now)?;
        }

        let stored = get_in(transaction, id)?.ok_or(Error::NotFound)?;
        Ok(stored)
    })
    .await
}

pub async fn rename(db: &Db, id: i64, name: &str) -> Result<Folder, Error> {
    update(
        db,
        id,
        FolderChange {
            name: Some(name.to_owned()),
            ..FolderChange::default()
        },
        0,
    )
    .await
}

/// Replace the topics a dynamic folder matches and return the new set.
pub async fn set_topics(
    db: &Db,
    id: i64,
    names: Vec<String>,
    now: i64,
) -> Result<Vec<Topic>, Error> {
    Ok(update(
        db,
        id,
        FolderChange {
            topics: Some(names),
            ..FolderChange::default()
        },
        now,
    )
    .await?
    .topics)
}

/// Move a folder under `parent` and put it at `position` among its siblings.
pub async fn place(db: &Db, id: i64, parent: Option<i64>, position: usize) -> Result<(), Error> {
    update(
        db,
        id,
        FolderChange {
            parent_id: Some(parent),
            position: Some(position),
            ..FolderChange::default()
        },
        0,
    )
    .await?;
    Ok(())
}

/// Delete a folder and its children, leaving their bookmarks unfiled.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.transaction(move |transaction| {
        let changed = transaction.execute("DELETE FROM folder WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// Check that a folder takes filed bookmarks, which a dynamic one does not.
pub(super) fn file_into(connection: &Connection, id: i64) -> Result<(), Error> {
    match kind(connection, id)? {
        Kind::Manual => Ok(()),
        Kind::Dynamic => Err(Error::Invalid(
            "a dynamic folder cannot hold filed bookmarks".to_owned(),
        )),
    }
}

pub(super) fn kind(connection: &Connection, id: i64) -> Result<Kind, Error> {
    match get_in(connection, id)? {
        Some(folder) => Ok(folder.kind),
        None => Err(Error::NotFound),
    }
}

fn replace_topics(
    connection: &Connection,
    id: i64,
    names: &[String],
    now: i64,
) -> Result<(), Error> {
    if kind(connection, id)? != Kind::Dynamic {
        return Err(Error::Invalid(
            "only a dynamic folder matches topics".to_owned(),
        ));
    }

    let wanted = topics::resolve(connection, names, now)?;
    let existing: Vec<i64> = {
        let mut statement =
            connection.prepare("SELECT topic_id FROM folder_topic WHERE folder_id = ?1")?;
        let rows = statement.query_map([id], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    for topic_id in &existing {
        if !wanted.iter().any(|topic| topic.id == *topic_id) {
            connection.execute(
                "DELETE FROM folder_topic WHERE folder_id = ?1 AND topic_id = ?2",
                params![id, topic_id],
            )?;
        }
    }
    for topic in &wanted {
        if !existing.contains(&topic.id) {
            connection.execute(
                "INSERT INTO folder_topic (folder_id, topic_id) VALUES (?1, ?2)",
                params![id, topic.id],
            )?;
        }
    }
    Ok(())
}

fn get_in(connection: &Connection, id: i64) -> Result<Option<Folder>, Error> {
    let sql = format!("SELECT {COLUMNS} FROM folder WHERE id = ?1");
    match connection.query_row(&sql, [id], from_row).optional()? {
        Some(mut folder) => {
            folder.topics = topics_of_folder(connection, id)?;
            Ok(Some(folder))
        }
        None => Ok(None),
    }
}

/// Whether `folder` is `parent` or somewhere below it.
fn inside(connection: &Connection, folder: i64, parent: i64) -> Result<bool, Error> {
    Ok(connection.query_row(
        "WITH RECURSIVE ancestor (id) AS (
             SELECT ?2
             UNION
             SELECT f.parent_id FROM folder f JOIN ancestor a ON f.id = a.id
             WHERE f.parent_id IS NOT NULL
         )
         SELECT EXISTS (SELECT 1 FROM ancestor WHERE id = ?1)",
        params![folder, parent],
        |row| row.get(0),
    )?)
}

/// The topics one folder matches, by name.
fn topics_of_folder(connection: &Connection, id: i64) -> Result<Vec<Topic>, Error> {
    let mut statement = connection.prepare(
        "SELECT t.id, t.name, t.created_at, t.followed_at
         FROM folder_topic ft JOIN topic t ON t.id = ft.topic_id
         WHERE ft.folder_id = ?1
         ORDER BY t.name",
    )?;
    let topics = statement.query_map([id], topics::from_row)?;
    Ok(topics.collect::<Result<Vec<_>, _>>()?)
}

/// The topics of every folder, keyed by folder id.
fn all_topics(connection: &Connection) -> Result<HashMap<i64, Vec<Topic>>, Error> {
    let mut statement = connection.prepare(
        "SELECT t.id, t.name, t.created_at, t.followed_at, ft.folder_id
         FROM folder_topic ft JOIN topic t ON t.id = ft.topic_id
         ORDER BY ft.folder_id, t.name",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, i64>(4)?, topics::from_row(row)?))
    })?;

    let mut topics: HashMap<i64, Vec<Topic>> = HashMap::new();
    for row in rows {
        let (folder, topic) = row?;
        topics.entry(folder).or_default().push(topic);
    }
    Ok(topics)
}

/// Read one folder row, without its topics.
fn from_row(row: &Row<'_>) -> rusqlite::Result<Folder> {
    Ok(Folder {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
        kind: named(row, 3, "folder kind", Kind::parse)?,
        position: row.get(4)?,
        created_at: row.get(5)?,
        topics: Vec::new(),
    })
}
