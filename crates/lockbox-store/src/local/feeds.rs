//! Feeds the reader follows, and the state of their last refresh.

use rusqlite::{OptionalExtension, Row, params, params_from_iter, types::Value};

use lockbox_types::{
    feed::{
        DEFAULT_REFRESH_SECONDS, DEFAULT_TIMEOUT_SECONDS, Feed, FeedChange, Kind, NewFeed, Refresh,
        Source,
    },
    visibility::Visibility,
};

use super::{entries, named};
use crate::{Db, Error};

/// Columns every feed query returns, in the order [`from_row`] reads them.
const COLUMNS: &str = "id, url, site_url, title, kind, refresh_seconds, timeout_seconds, user_agent, \
     next_refresh_at, fetched_at, etag, last_modified, last_error, visibility, disabled, \
     discovered_from, created_at, updated_at";

/// Position in the feed list from which the next page starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// Title of the last feed on the page, or its URL when it has none.
    pub name: String,
    /// Row identifier of that feed.
    pub id: i64,
}

/// One page of feeds.
pub type Page = super::Page<Feed, Cursor>;

/// Add a feed, or return the one already stored under `url`.
pub async fn add(db: &Db, feed: NewFeed, now: i64) -> Result<Feed, Error> {
    db.transaction(move |transaction| {
        transaction.execute(
            "INSERT INTO feed (url, site_url, title, kind, refresh_seconds, timeout_seconds,
                               user_agent, discovered_from, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT (url) DO NOTHING",
            params![
                feed.url,
                feed.site_url,
                feed.title,
                feed.kind.unwrap_or(Kind::Rss).as_str(),
                feed.refresh_seconds.unwrap_or(DEFAULT_REFRESH_SECONDS),
                feed.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS),
                feed.user_agent,
                feed.discovered_from.as_str(),
                now,
            ],
        )?;

        let sql = format!("SELECT {COLUMNS} FROM feed WHERE url = ?1");
        let stored = transaction.query_row(&sql, [&feed.url], from_row)?;
        Ok(stored)
    })
    .await
}

/// Read one feed.
pub async fn get(db: &Db, id: i64) -> Result<Option<Feed>, Error> {
    db.call(move |connection| {
        let sql = format!("SELECT {COLUMNS} FROM feed WHERE id = ?1");
        Ok(connection.query_row(&sql, [id], from_row).optional()?)
    })
    .await
}

/// The feeds whose next refresh is due, oldest first.
///
/// A feed which was never refreshed has no time to compare, so it counts as due.
pub async fn due(db: &Db, now: i64, limit: usize) -> Result<Vec<Feed>, Error> {
    db.call(move |connection| {
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM feed
             WHERE disabled = 0 AND (next_refresh_at IS NULL OR next_refresh_at <= ?1)
             ORDER BY next_refresh_at, id LIMIT ?2"
        ))?;
        let mut items = Vec::new();
        let mut rows = statement.query(params![now, limit.min(i64::MAX as usize) as i64])?;
        while let Some(row) = rows.next()? {
            items.push(from_row(row)?);
        }
        Ok(items)
    })
    .await
}

/// List feeds by title, with the URL standing in for a missing one.
///
/// `limit` is however many the caller wants; the store sets no maximum.
pub async fn list(db: &Db, after: Option<Cursor>, limit: usize) -> Result<Page, Error> {
    db.call(move |connection| {
        if limit == 0 {
            return Ok(Page {
                items: Vec::new(),
                next: None,
            });
        }
        // How many feeds a client shows is its decision, so there is no cap here
        let wanted = limit.min(i64::MAX as usize) as i64;

        let mut sql = format!("SELECT {COLUMNS} FROM feed");
        let mut args: Vec<Value> = Vec::new();
        if let Some(cursor) = after {
            sql.push_str(
                " WHERE (coalesce(title, url) COLLATE NOCASE > ?1
                   OR (coalesce(title, url) COLLATE NOCASE = ?1 AND id > ?2))",
            );
            args.push(Value::Text(cursor.name));
            args.push(Value::Integer(cursor.id));
        }
        sql.push_str(" ORDER BY coalesce(title, url) COLLATE NOCASE, id LIMIT ?");
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
                name: last.name().to_owned(),
                id: last.id,
            })
        } else {
            None
        };
        Ok(Page { items, next })
    })
    .await
}

/// Record a successful refresh and store the entries the feed did not have yet.
///
/// Returns how many entries were new.
pub async fn record_refresh(db: &Db, id: i64, refresh: Refresh) -> Result<usize, Error> {
    db.transaction(move |transaction| {
        let changed = transaction.execute(
            "UPDATE feed
             SET fetched_at = ?1, etag = ?2, last_modified = ?3,
                 next_refresh_at = ?1 + refresh_seconds, last_error = NULL, updated_at = ?1
             WHERE id = ?4",
            params![refresh.fetched_at, refresh.etag, refresh.last_modified, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }

        let inserted = entries::insert(transaction, id, &refresh.entries, refresh.fetched_at)?;
        Ok(inserted)
    })
    .await
}

/// Record a failed refresh, which leaves the validators and the entries alone.
pub async fn record_failure(db: &Db, id: i64, error: &str, now: i64) -> Result<(), Error> {
    let error = error.to_owned();
    db.call(move |connection| {
        let changed = connection.execute(
            "UPDATE feed
             SET fetched_at = ?1, next_refresh_at = ?1 + refresh_seconds, last_error = ?2,
                 updated_at = ?1
             WHERE id = ?3",
            params![now, error, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// Change the settings of one feed and return it.
///
/// A shorter or longer interval moves a refresh that was already scheduled to
/// `now + refresh_seconds`.
pub async fn update(db: &Db, id: i64, change: FeedChange, now: i64) -> Result<Feed, Error> {
    db.call(move |connection| {
        let mut sets = vec!["updated_at = ?"];
        let mut args = vec![Value::Integer(now)];

        if let Some(url) = change.url {
            sets.push("url = ?");
            args.push(Value::Text(url));
        }
        if let Some(site_url) = change.site_url {
            sets.push("site_url = ?");
            args.push(Value::Text(site_url));
        }
        if let Some(title) = change.title {
            sets.push("title = ?");
            args.push(Value::Text(title));
        }
        if let Some(kind) = change.kind {
            sets.push("kind = ?");
            args.push(Value::Text(kind.as_str().to_owned()));
        }
        if let Some(seconds) = change.refresh_seconds {
            sets.push("refresh_seconds = ?");
            args.push(Value::Integer(seconds));
            sets.push(
                "next_refresh_at = CASE WHEN next_refresh_at IS NULL THEN NULL ELSE ? + ? END",
            );
            args.push(Value::Integer(now));
            args.push(Value::Integer(seconds));
        }
        if let Some(visibility) = change.visibility {
            sets.push("visibility = ?");
            args.push(Value::Text(visibility.as_str().to_owned()));
        }
        if let Some(seconds) = change.timeout_seconds {
            sets.push("timeout_seconds = ?");
            args.push(Value::Integer(seconds));
        }
        if let Some(user_agent) = change.user_agent {
            sets.push("user_agent = ?");
            args.push(match user_agent {
                Some(user_agent) => Value::Text(user_agent),
                None => Value::Null,
            });
        }
        if let Some(disabled) = change.disabled {
            sets.push("disabled = ?");
            args.push(Value::Integer(i64::from(disabled)));
        }

        let sql = format!(
            "UPDATE feed SET {} WHERE id = ? RETURNING {COLUMNS}",
            sets.join(", ")
        );
        args.push(Value::Integer(id));

        match connection
            .query_row(&sql, params_from_iter(args), from_row)
            .optional()?
        {
            Some(feed) => Ok(feed),
            None => Err(Error::NotFound),
        }
    })
    .await
}

/// Remove a feed and every entry which comes from it.
pub async fn delete(db: &Db, id: i64) -> Result<(), Error> {
    db.call(move |connection| {
        let changed = connection.execute("DELETE FROM feed WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(Error::NotFound);
        }
        Ok(())
    })
    .await
}

/// Read one feed row.
fn from_row(row: &Row<'_>) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: row.get(0)?,
        url: row.get(1)?,
        site_url: row.get(2)?,
        title: row.get(3)?,
        kind: named(row, 4, "feed kind", Kind::parse)?,
        refresh_seconds: row.get(5)?,
        timeout_seconds: row.get(6)?,
        user_agent: row.get(7)?,
        next_refresh_at: row.get(8)?,
        fetched_at: row.get(9)?,
        etag: row.get(10)?,
        last_modified: row.get(11)?,
        last_error: row.get(12)?,
        visibility: named(row, 13, "feed visibility", Visibility::parse)?,
        disabled: row.get(14)?,
        discovered_from: named(row, 15, "feed source", Source::parse)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}
