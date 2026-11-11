//! Fetching feeds into the store.

use std::{
    fmt,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use lockbox_core::{
    feed::{self, Parsed},
    fetch::{Fetched, Fetcher, Request},
};
use lockbox_proto::api::reader::{self, DEFAULT_USER_AGENT, Kind};
use lockbox_store::{
    Db, Error as StoreError,
    local::{
        entries,
        feeds::{self, DEFAULT_TIMEOUT_SECONDS, NewFeed, Refresh},
    },
};

/// What a refresh of one feed found.
pub struct Refreshed {
    /// The feed after the refresh.
    pub feed: feeds::Feed,
    /// Entries which the refresh added.
    pub new_entries: usize,
}

/// Fetch one feed and store what it brought.
///
/// A failure is written into the feed's row, where the reader shows it until
/// the next refresh succeeds.
pub async fn refresh(db: &Db, fetcher: &Fetcher, feed: feeds::Feed) -> Result<Refreshed, Error> {
    let request = fetch_request(
        &feed.url,
        feed.etag.as_deref(),
        feed.last_modified.as_deref(),
        feed.timeout_seconds,
        feed.user_agent.as_deref(),
    );
    let fetched = match fetcher.fetch(&request).await {
        Ok(fetched) => fetched,
        Err(source) => {
            let error = Error::Fetch {
                url: feed.url.clone(),
                source,
            };
            fail(db, &feed, &error).await?;
            return Err(error);
        }
    };

    let entries = if fetched.unchanged {
        Vec::new()
    } else {
        match feed::parse(&fetched.body, Some(&fetched.url)) {
            Ok(parsed) => entries_of(parsed),
            Err(source) => {
                let error = Error::Parse {
                    url: fetched.url.to_string(),
                    source,
                };
                fail(db, &feed, &error).await?;
                return Err(error);
            }
        }
    };

    let new_entries = feeds::record_refresh(
        db,
        feed.id,
        Refresh {
            fetched_at: now(),
            etag: fetched.etag,
            last_modified: fetched.last_modified,
            entries,
        },
    )
    .await?;
    Ok(Refreshed {
        feed: stored(db, feed.id).await?,
        new_entries,
    })
}

/// Add the feed a user asked for, storing it with the items it brought.
pub async fn add(db: &Db, fetcher: &Fetcher, wanted: reader::NewFeed) -> Result<Refreshed, Error> {
    let timeout_seconds = wanted.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let (fetched, parsed) = resolve(
        fetcher,
        &wanted.url,
        timeout_seconds,
        wanted.user_agent.as_deref(),
    )
    .await?;

    let mut request = NewFeed::new(fetched.url.to_string());
    request.site_url = parsed.site_url.clone();
    request.title = wanted.title.or_else(|| parsed.title.clone());
    request.kind = kind(parsed.kind);
    request.timeout_seconds = timeout_seconds;
    request.user_agent = wanted.user_agent;
    if let Some(seconds) = wanted.refresh_seconds {
        request.refresh_seconds = seconds;
    }
    let feed = feeds::add(db, request, now()).await?;

    let new_entries = feeds::record_refresh(
        db,
        feed.id,
        Refresh {
            fetched_at: now(),
            etag: fetched.etag,
            last_modified: fetched.last_modified,
            entries: entries_of(parsed),
        },
    )
    .await?;
    Ok(Refreshed {
        feed: stored(db, feed.id).await?,
        new_entries,
    })
}

/// Fetch `url` and read it as a feed, or as a page which advertises one.
async fn resolve(
    fetcher: &Fetcher,
    url: &str,
    timeout_seconds: i64,
    user_agent: Option<&str>,
) -> Result<(Fetched, Parsed), Error> {
    let page = fetcher
        .fetch(&fetch_request(url, None, None, timeout_seconds, user_agent))
        .await
        .map_err(|error| Error::Fetch {
            url: url.to_owned(),
            source: error,
        })?;

    if let Ok(parsed) = feed::parse(&page.body, Some(&page.url)) {
        return Ok((page, parsed));
    }

    let advertised =
        feed::discover(&page.text(), &page.url).ok_or_else(|| Error::NoFeed(url.to_owned()))?;
    let fetched = fetcher
        .fetch(&fetch_request(
            advertised.as_str(),
            None,
            None,
            timeout_seconds,
            user_agent,
        ))
        .await
        .map_err(|error| Error::Fetch {
            url: advertised.to_string(),
            source: error,
        })?;
    let parsed = feed::parse(&fetched.body, Some(&fetched.url))
        .map_err(|_| Error::NoFeed(advertised.to_string()))?;
    Ok((fetched, parsed))
}

/// Write a failed refresh into the feed's row, which the reader can show.
async fn fail(db: &Db, feed: &feeds::Feed, error: &Error) -> Result<(), Error> {
    feeds::record_failure(db, feed.id, &error.to_string(), now()).await?;
    Ok(())
}

/// Read back a feed which a call just wrote.
async fn stored(db: &Db, id: i64) -> Result<feeds::Feed, Error> {
    match feeds::get(db, id).await? {
        Some(feed) => Ok(feed),
        None => Err(Error::Store(StoreError::NotFound)),
    }
}

/// The request for one fetch, with the feed's timeout and user agent.
fn fetch_request<'a>(
    url: &'a str,
    etag: Option<&'a str>,
    last_modified: Option<&'a str>,
    timeout_seconds: i64,
    user_agent: Option<&'a str>,
) -> Request<'a> {
    Request {
        url,
        etag,
        last_modified,
        timeout: Duration::from_secs(timeout_seconds.max(1) as u64),
        user_agent: user_agent.unwrap_or(DEFAULT_USER_AGENT),
    }
}

/// The entries which a document holds, in the store's terms.
fn entries_of(parsed: Parsed) -> Vec<entries::NewEntry> {
    parsed
        .entries
        .into_iter()
        .map(|item| entries::NewEntry {
            source: item.source,
            canonical_url: item.url.clone(),
            url: item.url,
            title: item.title,
            author: item.author,
            summary: item.summary,
            body: item.body,
            truncated: false,
            published_at: item.published_at,
        })
        .collect()
}

/// The feed format which the store keeps.
fn kind(kind: Kind) -> feeds::Kind {
    match kind {
        Kind::Rss => feeds::Kind::Rss,
        Kind::Atom => feeds::Kind::Atom,
        Kind::Json => feeds::Kind::Json,
    }
}

/// Time in seconds since the UNIX epoch.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

/// Why a feed could not be refreshed.
#[derive(Debug)]
pub enum Error {
    /// The document could not be fetched.
    Fetch {
        /// Address which was fetched.
        url: String,
        /// What went wrong.
        source: lockbox_core::fetch::Error,
    },
    /// The document could not be read as a feed.
    Parse {
        /// Address the document came from.
        url: String,
        /// What went wrong.
        source: feed::Error,
    },
    /// The address is neither a feed nor a page which advertises one.
    NoFeed(String),
    /// The store refused the call.
    Store(StoreError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch { url, source } => write!(f, "cannot fetch {url}: {source}"),
            Self::Parse { url, source } => write!(f, "cannot read the feed at {url}: {source}"),
            Self::NoFeed(url) => write!(f, "{url} is neither a feed nor a page with one"),
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fetch { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Store(error) => Some(error),
            Self::NoFeed(_) => None,
        }
    }
}

impl From<StoreError> for Error {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
