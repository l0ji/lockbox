//! Feeds the reader follows, and the state of their last refresh.

use rusqlite::{
    OptionalExtension, Row, params, params_from_iter,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, Value, ValueRef},
};

use super::{Visibility, entries, unknown};
use crate::{Db, Error};

/// Columns every feed query returns, in the order [`from_row`] reads them.
const COLUMNS: &str = "id, url, site_url, title, kind, refresh_seconds, timeout_seconds, user_agent, \
     next_refresh_at, fetched_at, etag, last_modified, last_error, visibility, disabled, \
     discovered_from, created_at, updated_at";

/// Seconds a fetch waits for the server when the user names no timeout.
pub const DEFAULT_TIMEOUT_SECONDS: i64 = 20;

/// One feed the reader follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub site_url: Option<String>,
    pub title: Option<String>,
    pub kind: Kind,
    pub refresh_seconds: i64,
    pub timeout_seconds: i64,
    pub user_agent: Option<String>,
    pub next_refresh_at: Option<i64>,
    pub fetched_at: Option<i64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub last_error: Option<String>,
    pub visibility: Visibility,
    pub disabled: bool,
    pub discovered_from: Source,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Feed {
    /// Title used in the list, falling back to the URL.
    pub fn name(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.url)
    }
}

/// Feed formats the reader can parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// RSS.
    Rss,
    /// Atom.
    Atom,
    /// JSON Feed.
    Json,
}

impl Kind {
    /// Name used in the database.
    fn text(self) -> &'static str {
        match self {
            Self::Rss => "rss",
            Self::Atom => "atom",
            Self::Json => "json",
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
            "rss" => Ok(Self::Rss),
            "atom" => Ok(Self::Atom),
            "json" => Ok(Self::Json),
            other => Err(unknown("feed kind", other)),
        }
    }
}

/// Where a feed was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Added by the user.
    User,
    /// Advertised by a site the reader looked at.
    Page,
    /// Named by another account's records.
    Federation,
}

impl Source {
    /// Name used in the database.
    fn text(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Page => "page",
            Self::Federation => "federation",
        }
    }
}

impl ToSql for Source {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(self.text().into())
    }
}

impl FromSql for Source {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "user" => Ok(Self::User),
            "page" => Ok(Self::Page),
            "federation" => Ok(Self::Federation),
            other => Err(unknown("feed source", other)),
        }
    }
}

/// A feed to add to the reader.
#[derive(Debug, Clone)]
pub struct NewFeed {
    /// URL of the feed itself.
    pub url: String,
    /// Site the feed belongs to, when it is known.
    pub site_url: Option<String>,
    /// Title to show in the reader.
    pub title: Option<String>,
    /// Format of the feed document.
    pub kind: Kind,
    /// Seconds between refreshes.
    pub refresh_seconds: i64,
    /// Seconds the server has to answer a fetch.
    pub timeout_seconds: i64,
    /// User agent a fetch sends, or the node's own when it is missing.
    pub user_agent: Option<String>,
    /// How the feed was found.
    pub discovered_from: Source,
}

impl NewFeed {
    /// A feed the user added themselves, refreshed every hour.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            site_url: None,
            title: None,
            kind: Kind::Rss,
            refresh_seconds: 3600,
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
            user_agent: None,
            discovered_from: Source::User,
        }
    }
}

/// What one successful fetch of a feed found.
#[derive(Debug, Clone)]
pub struct Refresh {
    /// When the feed was fetched.
    pub fetched_at: i64,
    /// Validator to send on the next fetch.
    pub etag: Option<String>,
    /// Date to send on the next fetch.
    pub last_modified: Option<String>,
    /// Entries the fetch parsed.
    pub entries: Vec<entries::NewEntry>,
}

/// Settings the user can change on a feed. A `None` leaves the field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    /// Address to fetch.
    pub url: Option<String>,
    /// Site the feed belongs to.
    pub site_url: Option<String>,
    /// Title to show in the reader.
    pub title: Option<String>,
    /// Format of the feed document.
    pub kind: Option<Kind>,
    /// Seconds between refreshes.
    pub refresh_seconds: Option<i64>,
    /// Seconds the server has to answer a fetch.
    pub timeout_seconds: Option<i64>,
    /// User agent a fetch sends, where `Some(None)` returns to the default.
    pub user_agent: Option<Option<String>>,
    /// Whether the feed is published.
    pub visibility: Option<Visibility>,
    /// Whether refreshing is paused.
    pub disabled: Option<bool>,
}

/// Add a feed, or return the one already stored under `url`.
pub async fn add(db: &Db, feed: NewFeed, now: i64) -> Result<Feed, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO feed (url, site_url, title, kind, refresh_seconds, timeout_seconds,
                               user_agent, discovered_from, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
             ON CONFLICT (url) DO NOTHING",
            params![
                feed.url,
                feed.site_url,
                feed.title,
                feed.kind,
                feed.refresh_seconds,
                feed.timeout_seconds,
                feed.user_agent,
                feed.discovered_from,
                now,
            ],
        )?;

        let sql = format!("SELECT {COLUMNS} FROM feed WHERE url = ?1");
        let stored = transaction.query_row(&sql, [&feed.url], from_row)?;
        transaction.commit()?;
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

/// Position in the feed list from which the next page starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// Title of the last feed on the page, or its URL when it has none.
    pub name: String,
    /// Row identifier of that feed.
    pub id: i64,
}

/// One page of feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Feed>,
    /// Where the next page starts, absent at the end of the list.
    pub next: Option<Cursor>,
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
    db.call(move |connection| {
        let transaction = connection.transaction()?;
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

        let inserted = entries::insert(&transaction, id, &refresh.entries, refresh.fetched_at)?;
        transaction.commit()?;
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
pub async fn update(db: &Db, id: i64, change: Change, now: i64) -> Result<Feed, Error> {
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
            args.push(Value::Text(kind.text().to_owned()));
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
            args.push(Value::Text(visibility.text().to_owned()));
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
        kind: row.get(4)?,
        refresh_seconds: row.get(5)?,
        timeout_seconds: row.get(6)?,
        user_agent: row.get(7)?,
        next_refresh_at: row.get(8)?,
        fetched_at: row.get(9)?,
        etag: row.get(10)?,
        last_modified: row.get(11)?,
        last_error: row.get(12)?,
        visibility: row.get(13)?,
        disabled: row.get(14)?,
        discovered_from: row.get(15)?,
        created_at: row.get(16)?,
        updated_at: row.get(17)?,
    })
}
