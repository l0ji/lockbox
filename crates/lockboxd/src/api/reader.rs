//! Routes for feeds, entries, bookmarks, folders and topics.

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
};
use lockbox_core::{canonical, settings::Settings};
use lockbox_proto::api::{
    ApiError, Cursor, DEFAULT_LIMIT, ListQuery, Page,
    cursor::Key,
    reader::{
        BOOKMARKS_ORDER_SORT, BOOKMARKS_PATH, BOOKMARKS_RECENT_SORT, BookmarkQuery,
        BookmarkRequest, ENTRIES_PATH, ENTRIES_SORT, EntryQuery, FEEDS_PATH, FEEDS_SORT,
        FOLDERS_PATH, SaveEntry, TOPICS_PATH, TOPICS_SORT, TopicsRequest,
    },
};
use lockbox_store::{
    Db, Error as StoreError,
    local::{bookmarks, entries, feeds, folders, settings, tags, topics},
};
use lockbox_types::{
    bookmark::{Bookmark, BookmarkChange, NewBookmark, Scope},
    entry::{Entry, EntryChange, EntrySummary},
    feed::{Feed, FeedChange, NewFeed, Refreshed},
    folder::{Folder, FolderChange, Kind, NewFolder},
    now,
    topic::{Topic, TopicChange},
    visibility::Visibility,
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
        .route(&format!("{ENTRIES_PATH}/{{id}}/save"), post(save_entry))
        .route(BOOKMARKS_PATH, get(list_bookmarks).post(save_bookmark))
        .route(
            &format!("{BOOKMARKS_PATH}/{{id}}"),
            get(read_bookmark)
                .patch(change_bookmark)
                .delete(remove_bookmark),
        )
        .route(
            &format!("{BOOKMARKS_PATH}/{{id}}/topics"),
            put(replace_topics),
        )
        .route(FOLDERS_PATH, get(list_folders).post(create_folder))
        .route(
            &format!("{FOLDERS_PATH}/{{id}}"),
            get(read_folder).patch(change_folder).delete(remove_folder),
        )
        .route(TOPICS_PATH, get(list_topics))
        .route(
            &format!("{TOPICS_PATH}/{{id}}"),
            get(read_topic).patch(change_topic).delete(remove_topic),
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

async fn save_entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(request): ApiJson<SaveEntry>,
) -> Result<(StatusCode, Json<Bookmark>), Failure> {
    let db = state.db();
    let entry = entries::get(db, id)
        .await?
        .ok_or_else(|| missing("entry"))?;
    let url = entry
        .summary
        .url
        .clone()
        .ok_or_else(|| Failure::new(ApiError::invalid("the entry has no link to save")))?;

    save(
        db,
        url,
        Some(entry.summary.title.clone()),
        request.note,
        request.folder_id,
        Some(id),
        request.topics,
        request.visibility,
    )
    .await
}

async fn list_bookmarks(
    State(state): State<AppState>,
    Query(query): Query<BookmarkQuery>,
) -> Result<Json<Page<Bookmark>>, Failure> {
    let db = state.db();
    let filter = query.filter()?;
    let sort = match filter.folder {
        Scope::Folder(id) => match folders::get(db, id).await? {
            Some(folder) if folder.kind == Kind::Manual => BOOKMARKS_ORDER_SORT,
            Some(_) => BOOKMARKS_RECENT_SORT,
            None => return Err(missing("folder")),
        },
        Scope::All | Scope::Unfiled => BOOKMARKS_RECENT_SORT,
    };
    let after = query
        .cursor
        .as_deref()
        .map(|text| bookmark_cursor_in(text, sort))
        .transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let page = bookmarks::list(db, &filter, after, limit).await?;

    let next = match page.next {
        Some(cursor) => Some(
            Cursor {
                sort: sort.to_owned(),
                key: vec![Key::Int(cursor.key)],
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

async fn save_bookmark(
    State(state): State<AppState>,
    ApiJson(request): ApiJson<BookmarkRequest>,
) -> Result<(StatusCode, Json<Bookmark>), Failure> {
    save(
        state.db(),
        request.url,
        request.title,
        request.note,
        request.folder_id,
        None,
        request.topics,
        request.visibility,
    )
    .await
}

async fn read_bookmark(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Bookmark>, Failure> {
    match bookmarks::get(state.db(), id).await? {
        Some(bookmark) => Ok(Json(bookmark)),
        None => Err(missing("bookmark")),
    }
}

async fn change_bookmark(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(mut change): ApiJson<BookmarkChange>,
) -> Result<Json<Bookmark>, Failure> {
    if let Some(url) = change.url.as_deref() {
        change.canonical_url = Some(
            canonical::canonical(url)
                .map_err(|error| Failure::new(ApiError::invalid(error.to_string())))?,
        );
    }
    Ok(Json(
        bookmarks::update(state.db(), id, change, now()).await?,
    ))
}

async fn remove_bookmark(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Failure> {
    bookmarks::delete(state.db(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn replace_topics(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(request): ApiJson<TopicsRequest>,
) -> Result<Json<Bookmark>, Failure> {
    let db = state.db();
    tags::set(db, id, request.topics, now()).await?;
    match bookmarks::get(db, id).await? {
        Some(bookmark) => Ok(Json(bookmark)),
        None => Err(missing("bookmark")),
    }
}

/// Lists folders as an ordered flat tree.
async fn list_folders(State(state): State<AppState>) -> Result<Json<Vec<Folder>>, Failure> {
    Ok(Json(folders::list(state.db()).await?))
}

async fn create_folder(
    State(state): State<AppState>,
    ApiJson(folder): ApiJson<NewFolder>,
) -> Result<(StatusCode, Json<Folder>), Failure> {
    let folder = folders::create(state.db(), folder, now()).await?;
    Ok((StatusCode::CREATED, Json(folder)))
}

async fn read_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Folder>, Failure> {
    match folders::get(state.db(), id).await? {
        Some(folder) => Ok(Json(folder)),
        None => Err(missing("folder")),
    }
}

async fn change_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(change): ApiJson<FolderChange>,
) -> Result<Json<Folder>, Failure> {
    Ok(Json(folders::update(state.db(), id, change, now()).await?))
}

async fn remove_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Failure> {
    folders::delete(state.db(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_topics(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Page<Topic>>, Failure> {
    let after = query.cursor.as_deref().map(topic_cursor_in).transpose()?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    let page = topics::page(state.db(), after, limit).await?;

    let next = match page.next {
        Some(cursor) => Some(
            Cursor {
                sort: TOPICS_SORT.to_owned(),
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

async fn read_topic(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Topic>, Failure> {
    match topics::get(state.db(), id).await? {
        Some(topic) => Ok(Json(topic)),
        None => Err(missing("topic")),
    }
}

async fn change_topic(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ApiJson(change): ApiJson<TopicChange>,
) -> Result<Json<Topic>, Failure> {
    let name = change
        .name
        .ok_or_else(|| Failure::new(ApiError::invalid("a topic needs a name")))?;
    Ok(Json(topics::rename(state.db(), id, &name).await?))
}

async fn remove_topic(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, Failure> {
    topics::delete(state.db(), id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[allow(clippy::too_many_arguments)]
async fn save(
    db: &Db,
    url: String,
    title: Option<String>,
    note: Option<String>,
    folder: Option<i64>,
    entry: Option<i64>,
    topics: Vec<String>,
    visibility: Option<Visibility>,
) -> Result<(StatusCode, Json<Bookmark>), Failure> {
    let canonical_url = canonical::canonical(&url)
        .map_err(|error| Failure::new(ApiError::invalid(error.to_string())))?;
    let title = title
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| canonical_url.clone());
    let visibility = match visibility {
        Some(visibility) => visibility,
        None => {
            let stored = db.call(|connection| settings::load(connection)).await?;
            let settings = Settings::from_stored(&stored)
                .map_err(|error| StoreError::Invalid(error.to_string()))?;
            if settings.publish_bookmarks {
                Visibility::Public
            } else {
                Visibility::Private
            }
        }
    };

    let saved = bookmarks::create(
        db,
        NewBookmark {
            url: url.trim().to_owned(),
            canonical_url,
            title,
            note: note.filter(|note| !note.trim().is_empty()),
            folder_id: folder,
            entry_id: entry,
            topics,
            visibility,
        },
        now(),
    )
    .await?;
    let status = if saved.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(saved.bookmark)))
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

fn bookmark_cursor_in(text: &str, sort: &str) -> Result<bookmarks::Cursor, ApiError> {
    let cursor = Cursor::decode(text, sort)?;
    match cursor.key.as_slice() {
        [Key::Int(key)] => Ok(bookmarks::Cursor {
            key: *key,
            id: cursor.id,
        }),
        _ => Err(ApiError::invalid(format!(
            "cursor {text} is not a position in a bookmark list"
        ))),
    }
}

fn topic_cursor_in(text: &str) -> Result<topics::Cursor, ApiError> {
    let cursor = Cursor::decode(text, TOPICS_SORT)?;
    match cursor.key.as_slice() {
        [Key::Text(name)] => Ok(topics::Cursor {
            name: name.clone(),
            id: cursor.id,
        }),
        _ => Err(ApiError::invalid(format!(
            "cursor {text} is not a position in the topic list"
        ))),
    }
}
