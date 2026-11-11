//! Routes for the feeds the user follows and the entries they read.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use lockbox_proto::api::{
    ApiError, Cursor, DEFAULT_LIMIT, ListQuery, Page,
    cursor::Key,
    reader::{
        ENTRIES_PATH, ENTRIES_SORT, Entry, EntryChange, EntryQuery, EntrySummary, FEEDS_PATH,
        FEEDS_SORT, Feed, FeedChange, Kind, NewFeed, Refreshed, Visibility,
    },
};
use lockbox_store::local::{entries, feeds};

use super::{ApiJson, Failure};
use crate::{
    AppState,
    reader::{self, now},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(FEEDS_PATH, get(list_feeds).post(add_feed))
        .route(
            &format!("{FEEDS_PATH}/{{id}}"),
            get(read_feed).patch(change_feed).delete(remove_feed),
        )
        .route(&format!("{FEEDS_PATH}/{{id}}/refresh"), post(refresh_feed))
        .route(ENTRIES_PATH, get(list_entries))
        .route(
            &format!("{ENTRIES_PATH}/{{id}}"),
            get(read_entry).patch(change_entry).delete(remove_entry),
        )
}

/// List the feeds, sorted by title with the URL standing in for a missing one.
async fn list_feeds(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<Feed>>, Failure> {
    let after = query.cursor.as_deref().map(feed_cursor_in).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let page = feeds::list(state.db(), after, limit).await?;

    let next = match page.next {
        Some(cursor) => Some(
            Cursor {
                sort: FEEDS_SORT.to_owned(),
                key: vec![Key::Text(cursor.name)],
                id: cursor.id,
            }
            .encode()?,
        ),
        None => None,
    };
    Ok(Json(Page {
        items: page.items.into_iter().map(feed_out).collect(),
        next,
    }))
}

/// Add a feed by address and refresh it once.
async fn add_feed(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<NewFeed>,
) -> Result<(StatusCode, Json<Refreshed>), Failure> {
    let refreshed = reader::add(state.db(), state.fetcher(), request)
        .await
        .map_err(refresh_failure)?;
    Ok((StatusCode::CREATED, Json(refreshed_out(refreshed))))
}

/// Read one feed.
async fn read_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Feed>, Failure> {
    match feeds::get(state.db(), id).await? {
        Some(feed) => Ok(Json(feed_out(feed))),
        None => Err(missing("feed")),
    }
}

/// Change the settings of one feed.
async fn change_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(change): ApiJson<FeedChange>,
) -> Result<Json<Feed>, Failure> {
    let change = feeds::Change {
        url: change.url,
        site_url: change.site_url,
        title: change.title,
        kind: change.kind.map(kind_in),
        refresh_seconds: change.refresh_seconds,
        timeout_seconds: change.timeout_seconds,
        user_agent: change.user_agent,
        visibility: change.visibility.map(visibility_in),
        disabled: change.disabled,
    };
    Ok(Json(feed_out(
        feeds::update(state.db(), id, change, now()).await?,
    )))
}

/// Remove one feed and the entries it brought.
async fn remove_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Failure> {
    feeds::delete(state.db(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Fetch one feed now.
async fn refresh_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Refreshed>, Failure> {
    let feed = feeds::get(state.db(), id)
        .await?
        .ok_or_else(|| missing("feed"))?;
    let refreshed = reader::refresh(state.db(), state.fetcher(), feed)
        .await
        .map_err(refresh_failure)?;
    Ok(Json(refreshed_out(refreshed)))
}

/// List the entries which a filter matches, newest first.
async fn list_entries(
    State(state): State<AppState>,
    Query(query): Query<EntryQuery>,
) -> Result<Json<Page<EntrySummary>>, Failure> {
    let filter = entries::Filter {
        feed_id: query.feed,
        read: query.read,
        starred: query.starred,
        archived: query.archived,
        query: query.query,
    };
    let after = query.cursor.as_deref().map(entry_cursor).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let page = entries::list(state.db(), &filter, after, limit).await?;

    let next = match page.next {
        Some(cursor) => Some(
            Cursor {
                sort: ENTRIES_SORT.to_owned(),
                key: vec![Key::Int(cursor.sort_at)],
                id: cursor.id,
            }
            .encode()?,
        ),
        None => None,
    };
    Ok(Json(Page {
        items: page.items.into_iter().map(summary_out).collect(),
        next,
    }))
}

/// Read one entry with its body.
async fn read_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Entry>, Failure> {
    match entries::get(state.db(), id).await? {
        Some(entry) => Ok(Json(entry_out(entry))),
        None => Err(missing("entry")),
    }
}

/// Change the reader state of one entry.
async fn change_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(change): ApiJson<EntryChange>,
) -> Result<Json<Entry>, Failure> {
    let db = state.db();
    if let Some(read) = change.read {
        entries::mark_read(db, id, read.then(now)).await?;
    }
    if let Some(starred) = change.starred {
        entries::mark_starred(db, id, starred.then(now)).await?;
    }
    if let Some(archived) = change.archived {
        entries::mark_archived(db, id, archived.then(now)).await?;
    }

    match entries::get(db, id).await? {
        Some(entry) => Ok(Json(entry_out(entry))),
        None => Err(missing("entry")),
    }
}

/// Delete one entry.
async fn remove_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Failure> {
    entries::delete(state.db(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A failure which names the row that does not exist.
fn missing(what: &str) -> Failure {
    Failure::new(ApiError {
        code: lockbox_proto::api::Code::NotFound,
        error: format!("no such {what}"),
    })
}

/// The reply for a failed refresh, which is a gateway error unless the store or
/// the address is at fault.
fn refresh_failure(error: reader::Error) -> Failure {
    match error {
        reader::Error::Store(error) => error.into(),
        reader::Error::NoFeed(_) => ApiError::invalid(error.to_string()).into(),
        reader::Error::Fetch { .. } | reader::Error::Parse { .. } => {
            Failure::new(ApiError::internal(error.to_string())).with_status(StatusCode::BAD_GATEWAY)
        }
    }
}

/// Position in the feed list which a cursor names.
fn feed_cursor_in(text: &str) -> Result<feeds::Cursor, ApiError> {
    let cursor = Cursor::decode(text, FEEDS_SORT)?;
    match cursor.key.as_slice() {
        [Key::Text(name)] => Ok(feeds::Cursor {
            name: name.clone(),
            id: cursor.id,
        }),
        _ => Err(ApiError::invalid(format!(
            "cursor {text} is not a position in the feed list"
        ))),
    }
}

/// Position in the reading list which a cursor names.
fn entry_cursor(text: &str) -> Result<entries::Cursor, ApiError> {
    let cursor = Cursor::decode(text, ENTRIES_SORT)?;
    match cursor.key.as_slice() {
        [Key::Int(sort_at)] => Ok(entries::Cursor {
            sort_at: *sort_at,
            id: cursor.id,
        }),
        _ => Err(ApiError::invalid(format!(
            "cursor {text} is not a position in the reading list"
        ))),
    }
}

/// The reply form of one feed.
fn feed_out(feed: feeds::Feed) -> Feed {
    Feed {
        id: feed.id,
        url: feed.url,
        site_url: feed.site_url,
        title: feed.title,
        kind: kind_out(feed.kind),
        refresh_seconds: feed.refresh_seconds,
        timeout_seconds: feed.timeout_seconds,
        user_agent: feed.user_agent,
        next_refresh_at: feed.next_refresh_at,
        fetched_at: feed.fetched_at,
        last_error: feed.last_error,
        visibility: visibility_out(feed.visibility),
        disabled: feed.disabled,
        created_at: feed.created_at,
        updated_at: feed.updated_at,
    }
}

/// The reply form of one refresh.
fn refreshed_out(refreshed: reader::Refreshed) -> Refreshed {
    Refreshed {
        feed: feed_out(refreshed.feed),
        new_entries: refreshed.new_entries,
    }
}

/// The reply form of one entry of a list.
fn summary_out(entry: entries::EntrySummary) -> EntrySummary {
    EntrySummary {
        id: entry.id,
        guid: entry.guid,
        feed_id: entry.feed_id,
        title: entry.title,
        url: entry.url,
        author: entry.author,
        summary: entry.summary,
        published_at: entry.published_at,
        sort_at: entry.sort_at,
        fetched_at: entry.fetched_at,
        read_at: entry.read_at,
        starred_at: entry.starred_at,
        archived_at: entry.archived_at,
    }
}

/// The reply form of one entry with its body.
fn entry_out(entry: entries::Entry) -> Entry {
    Entry {
        summary: summary_out(entry.summary),
        body: entry.body,
        truncated: entry.truncated,
    }
}

/// The reply form of a feed's format.
fn kind_out(kind: feeds::Kind) -> Kind {
    match kind {
        feeds::Kind::Rss => Kind::Rss,
        feeds::Kind::Atom => Kind::Atom,
        feeds::Kind::Json => Kind::Json,
    }
}

/// The stored form of a feed's format.
fn kind_in(kind: Kind) -> feeds::Kind {
    match kind {
        Kind::Rss => feeds::Kind::Rss,
        Kind::Atom => feeds::Kind::Atom,
        Kind::Json => feeds::Kind::Json,
    }
}

/// The reply form of a feed's visibility.
fn visibility_out(visibility: lockbox_store::local::Visibility) -> Visibility {
    match visibility {
        lockbox_store::local::Visibility::Public => Visibility::Public,
        lockbox_store::local::Visibility::Private => Visibility::Private,
    }
}

/// The stored form of a feed's visibility.
fn visibility_in(visibility: Visibility) -> lockbox_store::local::Visibility {
    match visibility {
        Visibility::Public => lockbox_store::local::Visibility::Public,
        Visibility::Private => lockbox_store::local::Visibility::Private,
    }
}
