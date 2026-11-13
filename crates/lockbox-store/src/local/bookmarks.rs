//! Links which are saved into the reader.

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use lockbox_types::{
    bookmark::{Bookmark, BookmarkChange, Filter, NewBookmark, Scope},
    visibility::Visibility,
};

use super::{Placed, folders, named, renumber, search_query, topics};
use crate::{Db, Error};

/// Columns that every bookmark query returns, in the order [`from_row`] reads
/// them.
const COLUMNS: &str = "id, url, canonical_url, title, note, folder_id, position, entry_id, \
     created_at, updated_at, archived_at, visibility";

/// A saved bookmark and whether it was newly created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub bookmark: Bookmark,
    pub created: bool,
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
pub type Page = super::Page<Bookmark, Cursor>;

/// Save a bookmark with its topics, or return the existing bookmark unchanged.
pub async fn create(db: &Db, bookmark: NewBookmark, now: i64) -> Result<Saved, Error> {
    db.transaction(move |transaction| {
        let title = bookmark.title.trim().to_owned();
        if title.is_empty() {
            return Err(Error::Invalid("a bookmark needs a title".to_owned()));
        }
        if bookmark.canonical_url.trim().is_empty() {
            return Err(Error::Invalid("a bookmark needs an address".to_owned()));
        }

        if let Some(folder) = bookmark.folder_id {
            folders::file_into(transaction, folder)?;
        }
        let changed = transaction.execute(
            "INSERT INTO bookmark (
                 url, canonical_url, title, note, folder_id, position, entry_id,
                 visibility, created_at, updated_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5,
                 (SELECT coalesce(max(position) + 1, 0) FROM bookmark WHERE folder_id IS ?5),
                 ?6, ?7, ?8, ?8)
             ON CONFLICT (canonical_url) DO NOTHING",
            params![
                bookmark.url,
                bookmark.canonical_url,
                title,
                bookmark.note,
                bookmark.folder_id,
                bookmark.entry_id,
                bookmark.visibility.as_str(),
                now,
            ],
        )?;
        let created = changed == 1;

        if created {
            let id: i64 = transaction.query_row(
                "SELECT id FROM bookmark WHERE canonical_url = ?1",
                [&bookmark.canonical_url],
                |row| row.get(0),
            )?;
            if !bookmark.topics.is_empty() {
                for topic in topics::resolve(transaction, &bookmark.topics, now)? {
                    transaction.execute(
                        "INSERT INTO bookmark_topic (bookmark_id, topic_id, tagged_at)
                         VALUES (?1, ?2, ?3)",
                        params![id, topic.id, now],
                    )?;
                }
            }
        }

        let mut stored = transaction
            .query_row(
                &format!("SELECT {COLUMNS} FROM bookmark WHERE canonical_url = ?1"),
                [&bookmark.canonical_url],
                from_row,
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        stored.topics = topics::of_bookmark(transaction, stored.id)?;
        Ok(Saved {
            bookmark: stored,
            created,
        })
    })
    .await
}

pub async fn get(db: &Db, id: i64) -> Result<Option<Bookmark>, Error> {
    db.call(move |connection| get_in(connection, id)).await
}

/// List matching bookmarks, newest first except within manually ordered folders.
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

pub async fn update(db: &Db, id: i64, change: BookmarkChange, now: i64) -> Result<Bookmark, Error> {
    db.transaction(move |transaction| {
        let current = get_in(transaction, id)?.ok_or(Error::NotFound)?;

        let mut sets: Vec<&str> = vec!["updated_at = ?"];
        let mut args: Vec<Value> = vec![Value::Integer(now)];

        if let Some(url) = change.url {
            let url = url.trim().to_owned();
            if url.is_empty() {
                return Err(Error::Invalid("a bookmark needs an address".to_owned()));
            }
            sets.push("url = ?");
            args.push(Value::Text(url.clone()));
            sets.push("canonical_url = ?");
            args.push(Value::Text(change.canonical_url.unwrap_or(url)));
        } else if let Some(canonical_url) = change.canonical_url {
            sets.push("canonical_url = ?");
            args.push(Value::Text(canonical_url));
        }

        if let Some(title) = change.title {
            let title = title.trim().to_owned();
            if title.is_empty() {
                return Err(Error::Invalid("a bookmark needs a title".to_owned()));
            }
            sets.push("title = ?");
            args.push(Value::Text(title));
        }
        if let Some(note) = change.note {
            sets.push("note = ?");
            args.push(match note {
                Some(note) if !note.trim().is_empty() => Value::Text(note),
                _ => Value::Null,
            });
        }
        if let Some(visibility) = change.visibility {
            sets.push("visibility = ?");
            args.push(Value::Text(visibility.as_str().to_owned()));
        }
        if let Some(archived) = change.archived {
            sets.push("archived_at = ?");
            args.push(match archived {
                true => Value::Integer(now),
                false => Value::Null,
            });
        }

        let folder = change.folder_id.unwrap_or(current.folder_id);
        let moved = change.folder_id.is_some() && folder != current.folder_id;
        if moved {
            if let Some(folder) = folder {
                folders::file_into(transaction, folder)?;
            }
            sets.push("folder_id = ?");
            args.push(match folder {
                Some(folder) => Value::Integer(folder),
                None => Value::Null,
            });
        }

        args.push(Value::Integer(id));
        transaction.execute(
            &format!("UPDATE bookmark SET {} WHERE id = ?", sets.join(", ")),
            params_from_iter(args),
        )?;

        if moved || change.position.is_some() {
            let position = match change.position {
                Some(position) => position,
                None => {
                    let count: i64 = transaction.query_row(
                        "SELECT count(*) FROM bookmark WHERE folder_id IS ?1 AND id <> ?2",
                        params![folder, id],
                        |row| row.get(0),
                    )?;
                    count as usize
                }
            };
            renumber(transaction, Placed::Bookmarks, folder, id, position)?;
        }

        let stored = get_in(transaction, id)?.ok_or(Error::NotFound)?;
        Ok(stored)
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
    update(
        db,
        id,
        BookmarkChange {
            folder_id: Some(folder),
            position: Some(position),
            ..BookmarkChange::default()
        },
        now,
    )
    .await?;
    Ok(())
}

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

fn get_in(connection: &Connection, id: i64) -> Result<Option<Bookmark>, Error> {
    match connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM bookmark WHERE id = ?1"),
            [id],
            from_row,
        )
        .optional()?
    {
        Some(mut bookmark) => {
            bookmark.topics = topics::of_bookmark(connection, id)?;
            Ok(Some(bookmark))
        }
        None => Ok(None),
    }
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
            true
        }
        Scope::Folder(folder) => match folders::kind(connection, folder)? {
            lockbox_types::folder::Kind::Manual => {
                sql.push_str(" AND b.folder_id = ?");
                args.push(Value::Integer(folder));
                false
            }
            lockbox_types::folder::Kind::Dynamic => {
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
    match filter.archived {
        Some(true) => sql.push_str(" AND b.archived_at IS NOT NULL"),
        Some(false) => sql.push_str(" AND b.archived_at IS NULL"),
        None => {}
    }
    if let Some(query) = filter.query.as_deref().and_then(search_query) {
        sql.push_str(
            " AND b.id IN (SELECT rowid FROM bookmark_search WHERE bookmark_search MATCH ?)",
        );
        args.push(Value::Text(query));
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

    let ids: Vec<i64> = items.iter().map(|bookmark| bookmark.id).collect();
    let mut topics = topics::of_bookmarks(connection, &ids)?;
    for bookmark in &mut items {
        bookmark.topics = topics.remove(&bookmark.id).unwrap_or_default();
    }

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
        visibility: named(row, 11, "bookmark visibility", Visibility::parse)?,
        topics: Vec::new(),
    })
}
