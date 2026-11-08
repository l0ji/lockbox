//! Links which are saved into the reader.

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use super::{
    Placed, Visibility,
    folders::{self, Kind},
    renumber,
};
use crate::{Db, Error};

/// Columns that every bookmark query returns, in the order [`from_row`] reads
/// them.
const COLUMNS: &str = "id, url, canonical_url, title, note, folder_id, position, entry_id, \
     created_at, updated_at, archived_at, visibility";

/// One saved link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub id: i64,
    pub url: String,
    /// Address that decides whether the link is already saved.
    pub canonical_url: String,
    pub title: String,
    pub note: Option<String>,
    pub folder_id: Option<i64>,
    pub position: i64,
    /// Entry the bookmark was saved from, if any.
    pub entry_id: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
    pub archived_at: Option<i64>,
    pub visibility: Visibility,
}

/// A bookmark to save.
#[derive(Debug, Clone)]
pub struct NewBookmark {
    pub url: String,
    pub canonical_url: String,
    pub title: String,
    pub note: Option<String>,
    pub folder_id: Option<i64>,
    pub entry_id: Option<i64>,
}

impl NewBookmark {
    /// A link saved under its own address.
    pub fn new(url: impl Into<String>, title: impl Into<String>) -> Self {
        let url = url.into();
        Self {
            canonical_url: url.clone(),
            url,
            title: title.into(),
            note: None,
            folder_id: None,
            entry_id: None,
        }
    }
}

/// Bookmark settings the user can change. A `None` leaves the field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub url: Option<String>,
    pub canonical_url: Option<String>,
    pub title: Option<String>,
    /// An empty note clears it.
    pub note: Option<String>,
    pub visibility: Option<Visibility>,
}

/// Which bookmarks a list looks at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Every bookmark.
    #[default]
    All,
    /// Bookmarks that are not in a folder.
    Unfiled,
    /// Bookmarks filed in or matched by one folder.
    Folder(i64),
}

/// Which bookmarks a list returns.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub folder: Scope,
    /// Only bookmarks which are carrying this topic.
    pub topic: Option<String>,
    /// Show only archived bookmarks when true.
    pub archived: Option<bool>,
    /// Words to match against the title, note and URL.
    pub query: Option<String>,
}

/// Position in a bookmark list that the next page starts after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Ordering key of the last bookmark: its `created_at` in the whole list,
    /// its `position` inside a folder.
    pub key: i64,
    pub id: i64,
}

/// One page of bookmarks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Bookmark>,
    /// Where the next page starts, absent at the end.
    pub next: Option<Cursor>,
}

/// Save a bookmark or return the one which is already saved under the same URL.
pub async fn create(db: &Db, bookmark: NewBookmark, now: i64) -> Result<Bookmark, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        if let Some(folder) = bookmark.folder_id {
            folders::file_into(&transaction, folder)?;
        }

        transaction.execute(
            "INSERT INTO bookmark (
                 url, canonical_url, title, note, folder_id, position, entry_id,
                 created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5,
                 (SELECT coalesce(max(position) + 1, 0) FROM bookmark WHERE folder_id IS ?5),
                 ?6, ?7, ?7)
             ON CONFLICT (canonical_url) DO NOTHING",
            params![
                bookmark.url,
                bookmark.canonical_url,
                bookmark.title,
                bookmark.note,
                bookmark.folder_id,
                bookmark.entry_id,
                now,
            ],
        )?;

        let sql = format!("SELECT {COLUMNS} FROM bookmark WHERE canonical_url = ?1");
        let stored = transaction.query_row(&sql, [&bookmark.canonical_url], from_row)?;
        transaction.commit()?;
        Ok(stored)
    })
    .await
}

/// Read one bookmark.
pub async fn get(db: &Db, id: i64) -> Result<Option<Bookmark>, Error> {
    db.call(move |connection| {
        let sql = format!("SELECT {COLUMNS} FROM bookmark WHERE id = ?1");
        Ok(connection.query_row(&sql, [id], from_row).optional()?)
    })
    .await
}

/// List the bookmarks a filter matches.
///
/// The whole list is newest first, a manual folder keeps its saved order and a
/// dynamic one lists what its topics match by recency. The cursor's key is
/// whatever the order is built on.
pub async fn list(
    db: &Db,
    filter: &Filter,
    after: Option<Cursor>,
    limit: usize,
) -> Result<Page, Error> {
    let filter = filter.clone();
    db.call(move |connection| page(connection, &filter, after, limit))
        .await
}

/// Change the settings of one bookmark and return it.
pub async fn update(db: &Db, id: i64, change: Change, now: i64) -> Result<Bookmark, Error> {
    db.call(move |connection| {
        let mut sets = vec!["updated_at = ?"];
        let mut args = vec![Value::Integer(now)];

        if let Some(url) = change.url {
            sets.push("url = ?");
            args.push(Value::Text(url));
        }
        if let Some(canonical_url) = change.canonical_url {
            sets.push("canonical_url = ?");
            args.push(Value::Text(canonical_url));
        }
        if let Some(title) = change.title {
            sets.push("title = ?");
            args.push(Value::Text(title));
        }
        if let Some(note) = change.note {
            sets.push("note = ?");
            args.push(match note.trim().is_empty() {
                true => Value::Null,
                false => Value::Text(note),
            });
        }
        if let Some(visibility) = change.visibility {
            sets.push("visibility = ?");
            args.push(Value::Text(visibility.text().to_owned()));
        }

        let sql = format!(
            "UPDATE bookmark SET {} WHERE id = ? RETURNING {COLUMNS}",
            sets.join(", ")
        );
        args.push(Value::Integer(id));

        match connection
            .query_row(&sql, params_from_iter(args), from_row)
            .optional()?
        {
            Some(bookmark) => Ok(bookmark),
            None => Err(Error::NotFound),
        }
    })
    .await
}

/// Move one bookmark into `folder` at `position` and renumber that folder.
pub async fn place(
    db: &Db,
    id: i64,
    folder: Option<i64>,
    position: usize,
    now: i64,
) -> Result<(), Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        if let Some(folder) = folder {
            folders::file_into(&transaction, folder)?;
        }

        let changed = transaction.execute(
            "UPDATE bookmark SET folder_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![folder, now, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }

        renumber(&transaction, Placed::Bookmarks, folder, id, position)?;
        transaction.commit()?;
        Ok(())
    })
    .await
}

/// Set or clear when a bookmark was archived.
pub async fn mark_archived(db: &Db, id: i64, at: Option<i64>) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute(
            "UPDATE bookmark SET archived_at = ?1 WHERE id = ?2",
            params![at, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// Remove one bookmark and the topics it carried.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute("DELETE FROM bookmark WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// One page of a filter's bookmarks.
fn page(
    connection: &Connection,
    filter: &Filter,
    after: Option<Cursor>,
    limit: usize,
) -> Result<Page, Error> {
    if limit == 0 {
        return Ok(Page {
            items: Vec::new(),
            next: None,
        });
    }
    // How many bookmarks a client shows is its decision, so there is no cap here
    let wanted = limit.min(i64::MAX as usize) as i64;

    let mut sql = format!("SELECT {COLUMNS} FROM bookmark b WHERE 1 = 1");
    let mut args: Vec<Value> = Vec::new();

    let recent = match filter.folder {
        Scope::All => true,
        Scope::Unfiled => {
            sql.push_str(" AND b.folder_id IS NULL");
            false
        }
        Scope::Folder(folder) => match folders::kind(connection, folder)? {
            Kind::Manual => {
                sql.push_str(" AND b.folder_id = ?");
                args.push(Value::Integer(folder));
                false
            }
            Kind::Dynamic => {
                sql.push_str(
                    " AND b.id IN (SELECT bt.bookmark_id FROM bookmark_topic bt
                     JOIN folder_topic ft ON ft.topic_id = bt.topic_id
                     WHERE ft.folder_id = ?)",
                );
                args.push(Value::Integer(folder));
                true
            }
        },
    };
    if let Some(topic) = &filter.topic
        && !topic.trim().is_empty()
    {
        sql.push_str(
            " AND b.id IN (SELECT bt.bookmark_id FROM bookmark_topic bt
             JOIN topic t ON t.id = bt.topic_id WHERE t.name = ?)",
        );
        args.push(Value::Text(topic.clone()));
    }
    if let Some(archived) = filter.archived {
        sql.push_str(if archived {
            " AND b.archived_at IS NOT NULL"
        } else {
            " AND b.archived_at IS NULL"
        });
    }
    if let Some(query) = &filter.query
        && !query.trim().is_empty()
    {
        sql.push_str(
            " AND b.id IN (SELECT rowid FROM bookmark_search WHERE bookmark_search MATCH ?)",
        );
        args.push(Value::Text(query.clone()));
    }
    if let Some(cursor) = after {
        sql.push_str(if recent {
            " AND (b.created_at < ? OR (b.created_at = ? AND b.id < ?))"
        } else {
            " AND (b.position > ? OR (b.position = ? AND b.id > ?))"
        });
        args.push(Value::Integer(cursor.key));
        args.push(Value::Integer(cursor.key));
        args.push(Value::Integer(cursor.id));
    }
    sql.push_str(if recent {
        " ORDER BY b.created_at DESC, b.id DESC LIMIT ?"
    } else {
        " ORDER BY b.position, b.id LIMIT ?"
    });
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
            key: match recent {
                true => last.created_at,
                false => last.position,
            },
            id: last.id,
        })
    } else {
        None
    };

    Ok(Page { items, next })
}

/// Read one bookmark row.
fn from_row(row: &Row<'_>) -> rusqlite::Result<Bookmark> {
    Ok(Bookmark {
        id: row.get(0)?,
        url: row.get(1)?,
        canonical_url: row.get(2)?,
        title: row.get(3)?,
        note: row.get(4)?,
        folder_id: row.get(5)?,
        position: row.get(6)?,
        entry_id: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        archived_at: row.get(10)?,
        visibility: row.get(11)?,
    })
}
