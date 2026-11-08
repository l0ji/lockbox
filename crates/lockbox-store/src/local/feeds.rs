//! Feeds the reader follows, and the state of their last poll.

use rusqlite::{
    OptionalExtension, Row, params, params_from_iter,
    types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, Value, ValueRef},
};

use super::{Visibility, entries, unknown};
use crate::{Db, Error};

/// Columns every feed query returns, in the order [`from_row`] reads them.
const COLUMNS: &str = "id, url, site_url, title, kind, poll_seconds, next_poll_at, \
     fetched_at, etag, last_modified, last_error, visibility, disabled, discovered_from, \
     created_at, updated_at";

/// One feed the reader follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub site_url: Option<String>,
    pub title: Option<String>,
    pub kind: Kind,
    pub poll_seconds: i64,
    pub next_poll_at: Option<i64>,
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
    /// Format the fetcher should expect.
    pub kind: Kind,
    /// Seconds between polls.
    pub poll_seconds: i64,
    /// How the feed was found.
    pub discovered_from: Source,
}

impl NewFeed {
    /// A feed the user added themselves, polled every hour.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            site_url: None,
            title: None,
            kind: Kind::Rss,
            poll_seconds: 3600,
            discovered_from: Source::User,
        }
    }
}

/// What one successful fetch of a feed found.
#[derive(Debug, Clone)]
pub struct Poll {
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
    /// Format to parse it as.
    pub kind: Option<Kind>,
    /// Seconds between polls.
    pub poll_seconds: Option<i64>,
    /// Whether the feed is published.
    pub visibility: Option<Visibility>,
    /// Whether polling is paused.
    pub disabled: Option<bool>,
}

/// Add a feed, or return the one already stored under `url`.
pub async fn add(db: &Db, feed: NewFeed, now: i64) -> Result<Feed, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO feed (url, site_url, title, kind, poll_seconds, discovered_from,
                               created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
             ON CONFLICT (url) DO NOTHING",
            params![
                feed.url,
                feed.site_url,
                feed.title,
                feed.kind,
                feed.poll_seconds,
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

/// Every feed, by title with the URL standing in for a missing one.
pub async fn list(db: &Db) -> Result<Vec<Feed>, Error> {
    db.call(|connection| {
        let sql =
            format!("SELECT {COLUMNS} FROM feed ORDER BY coalesce(title, url) COLLATE NOCASE, id");
        let mut statement = connection.prepare(&sql)?;
        let feeds = statement.query_map([], from_row)?;
        Ok(feeds.collect::<Result<Vec<_>, _>>()?)
    })
    .await
}

/// Record a successful poll and store the entries the feed did not have yet.
///
/// Returns how many entries were new.
pub async fn record_poll(db: &Db, id: i64, poll: Poll) -> Result<usize, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE feed
             SET fetched_at = ?1, etag = ?2, last_modified = ?3,
                 next_poll_at = ?1 + poll_seconds, last_error = NULL, updated_at = ?1
             WHERE id = ?4",
            params![poll.fetched_at, poll.etag, poll.last_modified, id],
        )?;
        if changed == 0 {
            return Err(Error::NotFound);
        }

        let inserted = entries::insert(&transaction, id, &poll.entries, poll.fetched_at)?;
        transaction.commit()?;
        Ok(inserted)
    })
    .await
}

/// Record a failed poll, which leaves the validators and the entries alone.
pub async fn record_failure(db: &Db, id: i64, error: &str, now: i64) -> Result<(), Error> {
    let error = error.to_owned();
    db.call(move |connection| {
        let changed = connection.execute(
            "UPDATE feed
             SET fetched_at = ?1, next_poll_at = ?1 + poll_seconds, last_error = ?2,
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
/// A shorter or longer interval moves a poll that was already scheduled to
/// `now + poll_seconds`.
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
        if let Some(seconds) = change.poll_seconds {
            sets.push("poll_seconds = ?");
            args.push(Value::Integer(seconds));
            sets.push("next_poll_at = CASE WHEN next_poll_at IS NULL THEN NULL ELSE ? + ? END");
            args.push(Value::Integer(now));
            args.push(Value::Integer(seconds));
        }
        if let Some(visibility) = change.visibility {
            sets.push("visibility = ?");
            args.push(Value::Text(visibility.text().to_owned()));
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

/// Read one feed row.
fn from_row(row: &Row<'_>) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: row.get(0)?,
        url: row.get(1)?,
        site_url: row.get(2)?,
        title: row.get(3)?,
        kind: row.get(4)?,
        poll_seconds: row.get(5)?,
        next_poll_at: row.get(6)?,
        fetched_at: row.get(7)?,
        etag: row.get(8)?,
        last_modified: row.get(9)?,
        last_error: row.get(10)?,
        visibility: row.get(11)?,
        disabled: row.get(12)?,
        discovered_from: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}
