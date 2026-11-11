//! Entries a feed brought, and the reader's view of them.

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter, types::Value};

use crate::{Db, Error};

/// Columns a list returns, in the order [`from_row`] reads them. The body is
/// never among them.
const COLUMNS: &str = "id, guid, feed_id, title, url, author, summary, published_at, sort_at, \
     fetched_at, read_at, starred_at, archived_at";

/// What a reader list shows for one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    pub id: i64,
    /// Identifier the store gave this entry.
    pub guid: String,
    pub feed_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub published_at: Option<i64>,
    /// Ordering key: the published date, or the fetch time when there is none.
    pub sort_at: i64,
    pub fetched_at: i64,
    pub read_at: Option<i64>,
    pub starred_at: Option<i64>,
    pub archived_at: Option<i64>,
}

/// One entry with its body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Everything a list shows.
    pub summary: EntrySummary,
    /// Body as the feed gave it.
    pub body: Option<String>,
    /// Whether the feed had already cut the body.
    pub truncated: bool,
}

/// Which entries a list returns. An empty filter returns every entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only entries from this feed.
    pub feed_id: Option<i64>,
    /// Only read entries when true, only unread ones when false.
    pub read: Option<bool>,
    /// Only starred entries when true, only unstarred ones when false.
    pub starred: Option<bool>,
    /// Only archived entries when true, only unarchived ones when false.
    pub archived: Option<bool>,
    /// Words to match against title, summary and body.
    pub query: Option<String>,
}

/// Position in the reading list that the next page starts after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Ordering key of the last entry on the page.
    pub sort_at: i64,
    /// Id of that entry.
    pub id: i64,
}

/// One page of the reading list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<EntrySummary>,
    /// Where the next page starts, absent at the end.
    pub next: Option<Cursor>,
}

/// An entry as a refresh found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEntry {
    /// Identifier the feed gives the item, or its URL when it has none.
    pub source: String,
    pub url: Option<String>,
    pub canonical_url: Option<String>,
    pub title: String,
    pub author: Option<String>,
    pub summary: Option<String>,
    /// Body as the feed gave it, kept whole.
    pub body: Option<String>,
    /// Whether the source itself had already cut the body.
    pub truncated: bool,
    pub published_at: Option<i64>,
}

/// List the entries a filter matches, newest first.
///
/// `limit` is however many the caller wants; the store sets no maximum.
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

/// Read one entry.
pub async fn get(db: &Db, id: i64) -> Result<Option<Entry>, Error> {
    db.call(move |connection| {
        let sql = format!("SELECT {COLUMNS}, body, truncated FROM entry WHERE id = ?1");
        Ok(connection.query_row(&sql, [id], detail).optional()?)
    })
    .await
}

/// Delete one entry and everything referencing it.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute("DELETE FROM entry WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// Set or clear when an entry was read.
pub async fn mark_read(db: &Db, id: i64, at: Option<i64>) -> Result<(), Error> {
    mark(db, Mark::Read, id, at).await
}

/// Set or clear when an entry was starred.
pub async fn mark_starred(db: &Db, id: i64, at: Option<i64>) -> Result<(), Error> {
    mark(db, Mark::Starred, id, at).await
}

/// Set or clear when an entry was archived.
pub async fn mark_archived(db: &Db, id: i64, at: Option<i64>) -> Result<(), Error> {
    mark(db, Mark::Archived, id, at).await
}

/// Store the entries a refresh found, skipping the ones the feed already had.
///
/// Returns how many were new.
pub(super) fn insert(
    connection: &Connection,
    feed_id: i64,
    entries: &[NewEntry],
    now: i64,
) -> Result<usize, Error> {
    let mut statement = connection.prepare_cached(
        "INSERT INTO entry (
             feed_id, guid, source, url, canonical_url, title, author, summary, body,
             truncated, published_at, sort_at, fetched_at, created_at, updated_at
         )
         VALUES (?1, lower(hex(randomblob(16))), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                 coalesce(?10, ?11), ?11, ?11, ?11)
         ON CONFLICT (feed_id, source) DO NOTHING",
    )?;

    let mut inserted = 0;
    for entry in entries {
        inserted += statement.execute(params![
            feed_id,
            entry.source,
            entry.url,
            entry.canonical_url,
            entry.title,
            entry.author,
            entry.summary,
            entry.body,
            entry.truncated,
            entry.published_at,
            now,
        ])?;
    }
    Ok(inserted)
}

/// Which reader state a mark changes.
#[derive(Debug, Clone, Copy)]
enum Mark {
    Read,
    Starred,
    Archived,
}

impl Mark {
    /// Column that holds the timestamp.
    fn column(self) -> &'static str {
        match self {
            Self::Read => "read_at",
            Self::Starred => "starred_at",
            Self::Archived => "archived_at",
        }
    }
}

/// Set one state column of one entry.
async fn mark(db: &Db, mark: Mark, id: i64, at: Option<i64>) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute(
            &format!("UPDATE entry SET {} = ?1 WHERE id = ?2", mark.column()),
            params![at, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// One page of a filter's entries.
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
    // How many entries a client shows is its decision, so there is no cap here
    let wanted = limit.min(i64::MAX as usize) as i64;

    let mut sql = format!("SELECT {COLUMNS} FROM entry e WHERE 1 = 1");
    let mut args: Vec<Value> = Vec::new();

    if let Some(feed) = filter.feed_id {
        sql.push_str(" AND e.feed_id = ?");
        args.push(Value::Integer(feed));
    }
    if let Some(read) = filter.read {
        sql.push_str(if read {
            " AND e.read_at IS NOT NULL"
        } else {
            " AND e.read_at IS NULL"
        });
    }
    if let Some(starred) = filter.starred {
        sql.push_str(if starred {
            " AND e.starred_at IS NOT NULL"
        } else {
            " AND e.starred_at IS NULL"
        });
    }
    if let Some(archived) = filter.archived {
        sql.push_str(if archived {
            " AND e.archived_at IS NOT NULL"
        } else {
            " AND e.archived_at IS NULL"
        });
    }
    if let Some(query) = &filter.query
        && !query.trim().is_empty()
    {
        sql.push_str(" AND e.id IN (SELECT rowid FROM entry_search WHERE entry_search MATCH ?)");
        args.push(Value::Text(query.clone()));
    }
    if let Some(cursor) = after {
        sql.push_str(" AND (e.sort_at < ? OR (e.sort_at = ? AND e.id < ?))");
        args.push(Value::Integer(cursor.sort_at));
        args.push(Value::Integer(cursor.sort_at));
        args.push(Value::Integer(cursor.id));
    }
    sql.push_str(" ORDER BY e.sort_at DESC, e.id DESC LIMIT ?");
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
            sort_at: last.sort_at,
            id: last.id,
        })
    } else {
        None
    };

    Ok(Page { items, next })
}

/// Read one entry with its body.
fn detail(row: &Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        summary: from_row(row)?,
        body: row.get(13)?,
        truncated: row.get(14)?,
    })
}

/// Read one entry of a list.
fn from_row(row: &Row<'_>) -> rusqlite::Result<EntrySummary> {
    Ok(EntrySummary {
        id: row.get(0)?,
        guid: row.get(1)?,
        feed_id: row.get(2)?,
        title: row.get(3)?,
        url: row.get(4)?,
        author: row.get(5)?,
        summary: row.get(6)?,
        published_at: row.get(7)?,
        sort_at: row.get(8)?,
        fetched_at: row.get(9)?,
        read_at: row.get(10)?,
        starred_at: row.get(11)?,
        archived_at: row.get(12)?,
    })
}
