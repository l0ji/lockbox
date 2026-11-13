//! Tests of the bookmarks, folder and topic pages.

#[path = "../common/mod.rs"]
mod common;

mod bookmark;
mod folders;
mod list;
mod topics;

use axum::{
    Json, Router,
    http::StatusCode,
    routing::{get, patch, put},
};
use lockbox_proto::api::{
    Page,
    node::SETTINGS_PATH,
    reader::{BOOKMARKS_PATH, FOLDERS_PATH, TOPICS_PATH},
};
use lockbox_types::{bookmark::Bookmark, folder::Folder};

use crate::common::{Record, sample_bookmark, sample_folder, sample_topic};

/// Records form requests and returns `reply` for creates.
pub(crate) fn stub(record: Record, reply: StatusCode) -> Router {
    let create = record.clone();
    let change = record.clone();
    let topics = record.clone();
    let remove = record.clone();
    let create_folder = record.clone();
    let change_folder = record.clone();
    let delete_folder = record.clone();
    let rename = record.clone();
    let delete_topic = record;

    Router::new()
        .route(
            BOOKMARKS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_bookmark()],
                    next: None,
                })
            })
            .post(move |body: String| {
                let record = create.clone();
                async move {
                    record.push(body);
                    (reply, Json(sample_bookmark()))
                }
            }),
        )
        .route(
            &format!("{BOOKMARKS_PATH}/{{id}}"),
            get(|| async { Json(sample_bookmark()) })
                .patch(move |body: String| {
                    let record = change.clone();
                    async move {
                        record.push(body);
                        Json(sample_bookmark())
                    }
                })
                .delete(move || {
                    let record = remove.clone();
                    async move {
                        record.push(String::new());
                        StatusCode::NO_CONTENT
                    }
                }),
        )
        .route(
            &format!("{BOOKMARKS_PATH}/{{id}}/topics"),
            put(move |body: String| {
                let record = topics.clone();
                async move {
                    record.push(body);
                    Json(sample_bookmark())
                }
            }),
        )
        .route(
            FOLDERS_PATH,
            get(folders).post(move |body: String| {
                let record = create_folder.clone();
                async move {
                    record.push(body);
                    (reply, Json(sample_folder()))
                }
            }),
        )
        .route(
            &format!("{FOLDERS_PATH}/{{id}}"),
            patch(move |body: String| {
                let record = change_folder.clone();
                async move {
                    record.push(body);
                    Json(sample_folder())
                }
            })
            .delete(move || {
                let record = delete_folder.clone();
                async move {
                    record.push(String::new());
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            TOPICS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_topic()],
                    next: None,
                })
            }),
        )
        .route(
            &format!("{TOPICS_PATH}/{{id}}"),
            patch(move |body: String| {
                let record = rename.clone();
                async move {
                    record.push(body);
                    Json(sample_topic())
                }
            })
            .delete(move || {
                let record = delete_topic.clone();
                async move {
                    record.push(String::new());
                    StatusCode::NO_CONTENT
                }
            }),
        )
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        )
}

/// Serves a root folder and its child.
pub(crate) async fn folders() -> Json<Vec<Folder>> {
    let mut child = sample_folder();
    child.id = 2;
    child.parent_id = Some(1);
    child.name = "Rust".to_owned();
    child.position = 0;
    Json(vec![sample_folder(), child])
}

/// Serves a bookmark page with folders, topics and settings.
pub(crate) fn list_stub(items: Vec<Bookmark>, next: Option<String>) -> Router {
    Router::new()
        .route(
            BOOKMARKS_PATH,
            get(move || {
                let items = items.clone();
                let next = next.clone();
                async move { Json(Page { items, next }) }
            }),
        )
        .route(FOLDERS_PATH, get(folders))
        .route(
            TOPICS_PATH,
            get(|| async {
                Json(Page {
                    items: vec![sample_topic()],
                    next: None,
                })
            }),
        )
        .route(
            SETTINGS_PATH,
            get(|| async { Json(serde_json::json!({ "publish_bookmarks": true })) }),
        )
}
