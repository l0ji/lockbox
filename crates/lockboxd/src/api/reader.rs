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
    reader::{ENTRIES_PATH, ENTRIES_SORT, EntryQuery, FEEDS_PATH, FEEDS_SORT},
};
use lockbox_store::local::{entries, feeds};
use lockbox_types::{
    entry::{Entry, EntryChange, EntrySummary},
    feed::{Feed, FeedChange, NewFeed, Refreshed},
    now,
};

use super::{ApiJson, Failure};
use crate::{AppState, reader};

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
        items: page.items,
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
    Ok((StatusCode::CREATED, Json(refreshed)))
}

/// Read one feed.
async fn read_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Feed>, Failure> {
    match feeds::get(state.db(), id).await? {
        Some(feed) => Ok(Json(feed)),
        None => Err(missing("feed")),
    }
}

/// Change the settings of one feed.
async fn change_feed(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(change): ApiJson<FeedChange>,
) -> Result<Json<Feed>, Failure> {
    Ok(Json(feeds::update(state.db(), id, change, now()).await?))
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
    Ok(Json(refreshed))
}

/// List the entries which a filter matches, newest first.
async fn list_entries(
    State(state): State<AppState>,
    Query(query): Query<EntryQuery>,
) -> Result<Json<Page<EntrySummary>>, Failure> {
    let filter = query.filter();
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
        items: page.items,
        next,
    }))
}

/// Read one entry with its body.
async fn read_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Entry>, Failure> {
    match entries::get(state.db(), id).await? {
        Some(entry) => Ok(Json(entry)),
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
        Some(entry) => Ok(Json(entry)),
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
