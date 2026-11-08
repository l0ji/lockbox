//! The folders which bookmarks are locally filed into or matched by.

use std::collections::HashMap;

use rusqlite::{
    Connection, OptionalExtension, Row, params,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef},
};

use super::{
    Placed, renumber,
    topics::{self, Topic},
    unknown,
};
use crate::{Db, Error};

/// Columns every folder query returns, in the order [`from_row`] reads them.
const COLUMNS: &str = "id, parent_id, name, kind, position, created_at";

/// What decides a folder's contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Bookmarks are filed into it by hand.
    Manual,
    /// It matches the topics it carries.
    Dynamic,
}

impl Kind {
    /// Name used in the database.
    fn text(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Dynamic => "dynamic",
        }
    }
}

impl ToSql for Kind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.text().into())
    }
}

impl FromSql for Kind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "manual" => Ok(Self::Manual),
            "dynamic" => Ok(Self::Dynamic),
            other => Err(unknown("folder kind", other)),
        }
    }
}

/// One folder in the reader's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: Kind,
    pub position: i64,
    pub created_at: i64,
    /// Topics a dynamic folder matches, by name.
    pub topics: Vec<Topic>,
}

/// A folder to create.
#[derive(Debug, Clone)]
pub struct NewFolder {
    pub parent: Option<i64>,
    pub name: String,
    /// What decides the folder's contents.
    pub kind: Kind,
    /// Topics a dynamic folder matches; empty for a manual one.
    pub topics: Vec<String>,
}

impl NewFolder {
    /// An empty manual folder.
    pub fn new(parent: Option<i64>, name: impl Into<String>) -> Self {
        Self {
            parent,
            name: name.into(),
            kind: Kind::Manual,
            topics: Vec::new(),
        }
    }
}

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

/// Add a folder at the end of its siblings.
pub async fn create(db: &Db, folder: NewFolder, now: i64) -> Result<Folder, Error> {
    db.call(move |connection| {
        if folder.kind == Kind::Manual && !folder.topics.is_empty() {
            return Err(Error::Invalid(
                "only a dynamic folder matches topics".to_owned(),
            ));
        }

        let transaction = connection.transaction()?;
        let position: i64 = transaction.query_row(
            "SELECT coalesce(max(position) + 1, 0) FROM folder WHERE parent_id IS ?1",
            params![folder.parent],
            |row| row.get(0),
        )?;
        let sql = format!(
            "INSERT INTO folder (parent_id, name, kind, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5) RETURNING {COLUMNS}"
        );
        let mut stored = transaction.query_row(
            &sql,
            params![folder.parent, folder.name, folder.kind, position, now],
            from_row,
        )?;

        if !folder.topics.is_empty() {
            for topic in topics::resolve(&transaction, &folder.topics, now)? {
                transaction.execute(
                    "INSERT INTO folder_topic (folder_id, topic_id) VALUES (?1, ?2)",
                    params![stored.id, topic.id],
                )?;
            }
            stored.topics = topics_of_folder(&transaction, stored.id)?;
        }

        transaction.commit()?;
        Ok(stored)
    })
    .await
}

/// Rename one folder.
pub async fn rename(db: &Db, id: i64, name: &str) -> Result<Folder, Error> {
    let name = name.to_owned();
    db.call(move |connection| {
        let sql = format!("UPDATE folder SET name = ?1 WHERE id = ?2 RETURNING {COLUMNS}");
        match connection
            .query_row(&sql, params![name, id], from_row)
            .optional()?
        {
            Some(mut folder) => {
                folder.topics = topics_of_folder(connection, id)?;
                Ok(folder)
            }
            None => Err(Error::NotFound),
        }
    })
    .await
}

/// Replace the topics a dynamic folder matches and return the new set.
///
/// Names are trimmed, matched case insensitively and deduplicated, and blank
/// ones are dropped.
pub async fn set_topics(
    db: &Db,
    id: i64,
    names: Vec<String>,
    now: i64,
) -> Result<Vec<Topic>, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        if kind(&transaction, id)? != Kind::Dynamic {
            return Err(Error::Invalid(
                "only a dynamic folder matches topics".to_owned(),
            ));
        }

        let wanted = topics::resolve(&transaction, &names, now)?;
        let existing: Vec<i64> = {
            let mut statement =
                transaction.prepare("SELECT topic_id FROM folder_topic WHERE folder_id = ?1")?;
            let rows = statement.query_map([id], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for topic_id in &existing {
            if !wanted.iter().any(|topic| topic.id == *topic_id) {
                transaction.execute(
                    "DELETE FROM folder_topic WHERE folder_id = ?1 AND topic_id = ?2",
                    params![id, topic_id],
                )?;
            }
        }
        for topic in &wanted {
            if !existing.contains(&topic.id) {
                transaction.execute(
                    "INSERT INTO folder_topic (folder_id, topic_id) VALUES (?1, ?2)",
                    params![id, topic.id],
                )?;
            }
        }

        let topics = topics_of_folder(&transaction, id)?;
        transaction.commit()?;
        Ok(topics)
    })
    .await
}

/// Move a folder under `parent` and put it at `position` among its siblings.
pub async fn place(db: &Db, id: i64, parent: Option<i64>, position: usize) -> Result<(), Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        if let Some(parent) = parent
            && inside(&transaction, id, parent)?
        {
            return Err(Error::Invalid(
                "a folder cannot move inside itself".to_owned(),
            ));
        }

        let changed = transaction.execute(
            "UPDATE folder SET parent_id = ?1 WHERE id = ?2",
            params![parent, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }

        renumber(&transaction, Placed::Folders, parent, id, position)?;
        transaction.commit()?;
        Ok(())
    })
    .await
}

/// Remove one folder; its child folders go with it and its bookmarks become
/// unfiled.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute("DELETE FROM folder WHERE id = ?1", [id])?;
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

/// What kind of folder `id` is.
pub(super) fn kind(connection: &Connection, id: i64) -> Result<Kind, Error> {
    match connection
        .query_row("SELECT kind FROM folder WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?
    {
        Some(kind) => Ok(kind),
        None => Err(Error::NotFound),
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
        kind: row.get(3)?,
        position: row.get(4)?,
        created_at: row.get(5)?,
        topics: Vec::new(),
    })
}
