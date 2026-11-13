//! Tests of the reader pages and the forms which they provide.

#[path = "../common/mod.rs"]
mod common;

mod entries;
mod feeds;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use lockbox_proto::api::{
    Page,
    node::SETTINGS_PATH,
    reader::{self, FOLDERS_PATH},
};
use lockbox_types::{
    entry::EntrySummary,
    feed::{Feed, Refreshed},
    folder::Folder,
};

use crate::common::{Record, sample_body, sample_entry, sample_feed};

/// Serves feeds, entries, folders and settings for the reader.
pub(crate) fn lists(feed: Feed, entries: Vec<EntrySummary>) -> Router {
    Router::new()
        .route(
            reader::FEEDS_PATH,
            get(move || {
                let feed = feed.clone();
                async move {
                    Json(Page {
                        items: vec![feed],
                        next: None,
                    })
                }
            }),
        )
        .route(
            reader::ENTRIES_PATH,
            get(move || {
                let entries = entries.clone();
                async move {
                    Json(Page {
                        items: entries.clone(),
                        next: None,
                    })
                }
            }),
        )
        .route(FOLDERS_PATH, get(|| async { Json(Vec::<Folder>::new()) }))
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        )
}

/// Records form requests and returns `reply` when adding a feed.
pub(crate) fn recorded(record: Record, reply: StatusCode) -> Router {
    let add = record.clone();
    let refresh = record.clone();
    let remove = record.clone();
    let settings = record.clone();
    let mark = record.clone();
    let save = record;

    Router::new()
        .route(
            reader::FEEDS_PATH,
            post(move |body: String| {
                let record = add.clone();
                async move {
                    record.push(body);
                    (
                        reply,
                        Json(Refreshed {
                            feed: sample_feed(),
                            new_entries: 2,
                        }),
                    )
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}/refresh",
            post(move |body: String| {
                let record = refresh.clone();
                async move {
                    record.push(body);
                    Json(Refreshed {
                        feed: sample_feed(),
                        new_entries: 1,
                    })
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}",
            delete(move || {
                let record = remove.clone();
                async move {
                    record.push(String::new());
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            "/api/reader/feeds/{id}",
            patch(move |body: String| {
                let record = settings.clone();
                async move {
                    record.push(body);
                    Json(sample_feed())
                }
            }),
        )
        .route(
            "/api/reader/entries/{id}",
            patch(move |body: String| {
                let record = mark.clone();
                async move {
                    record.push(body);
                    Json(sample_body())
                }
            }),
        )
        .route(
            "/api/reader/entries/{id}/save",
            post(move |body: String| {
                let record = save.clone();
                async move {
                    record.push(body);
                    StatusCode::CREATED
                }
            }),
        )
        .route(
            reader::ENTRIES_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_entry()],
                    next: None,
                })
            }),
        )
        .route(
            reader::FEEDS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_feed()],
                    next: None,
                })
            }),
        )
}
