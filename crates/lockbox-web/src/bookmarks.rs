//! The routes for the saved links, folders and topics.

use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use lockbox_proto::api::{
    ListQuery, Page,
    reader::{
        BOOKMARKS_PATH, BookmarkQuery, BookmarkRequest, FOLDERS_PATH, TOPICS_PATH, TopicsRequest,
        bookmark_path, bookmark_topics_path, folder_path, topic_path,
    },
};
use lockbox_types::{
    bookmark::{Bookmark, BookmarkChange},
    folder::{FolderChange, Kind, NewFolder},
    now,
    topic::{Topic, TopicChange},
    visibility::Visibility,
};
use serde::Deserialize;

use crate::{AppState, Failure, data, reader, reason, render, return_to};

/// Page which lists the saved links.
pub const BOOKMARKS_PAGE: &str = "/bookmarks";

/// Page which lists the archived links.
pub const ARCHIVED_PAGE: &str = "/bookmarks/archived";

/// Base of the page which edits a single saved link.
pub const BOOKMARK_PAGE: &str = "/bookmark";

/// Page which creates a saved link.
pub const NEW_BOOKMARK_PAGE: &str = "/bookmark/new";

/// Page which manages the folder tree.
pub const FOLDERS_PAGE: &str = "/folders";

/// Page which manages the topics.
pub const TOPICS_PAGE: &str = "/topics";

pub fn bookmark_page(id: i64) -> String {
    format!("{BOOKMARK_PAGE}/{id}")
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(BOOKMARKS_PAGE, get(list).post(create))
        .route(ARCHIVED_PAGE, get(archived))
        .route(NEW_BOOKMARK_PAGE, get(new_bookmark))
        .route(&format!("{BOOKMARK_PAGE}/{{id}}"), get(show).post(change))
        .route(&format!("{BOOKMARK_PAGE}/{{id}}/topics"), post(set_topics))
        .route(&format!("{BOOKMARK_PAGE}/{{id}}/archive"), post(archive))
        .route(&format!("{BOOKMARK_PAGE}/{{id}}/delete"), post(delete))
        .route(FOLDERS_PAGE, get(folders).post(create_folder))
        .route(
            &format!("{FOLDERS_PAGE}/{{id}}/change"),
            post(change_folder),
        )
        .route(
            &format!("{FOLDERS_PAGE}/{{id}}/delete"),
            post(delete_folder),
        )
        .route(TOPICS_PAGE, get(topics))
        .route(&format!("{TOPICS_PAGE}/{{id}}/rename"), post(rename_topic))
        .route(&format!("{TOPICS_PAGE}/{{id}}/delete"), post(delete_topic))
}

/// Bookmark list filters from the browser.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ListPageQuery {
    folder: Option<String>,
    topic: Option<String>,
    query: Option<String>,
    cursor: Option<String>,
}

async fn list(
    State(state): State<AppState>,
    Query(query): Query<ListPageQuery>,
) -> Result<Html<String>, Failure> {
    show_list(&state, &query, false).await
}

async fn archived(
    State(state): State<AppState>,
    Query(query): Query<ListPageQuery>,
) -> Result<Html<String>, Failure> {
    show_list(&state, &query, true).await
}

async fn show_list(
    state: &AppState,
    query: &ListPageQuery,
    archived: bool,
) -> Result<Html<String>, Failure> {
    let client = state.client();
    let wanted = BookmarkQuery {
        folder: query.folder.clone(),
        topic: query.topic.clone(),
        archived: Some(archived),
        query: query.query.clone(),
        limit: None,
        cursor: query.cursor.clone(),
    };
    let page: Page<Bookmark> = client
        .get_json(&format!("{BOOKMARKS_PATH}{}", wanted.to_query()))
        .await?;
    let folders = data::folders(client).await?;
    let topics = data::topics(client, 30).await?;
    let now = now();
    let rows = page
        .items
        .into_iter()
        .map(|bookmark| row(bookmark, now))
        .collect();
    let base = match archived {
        true => ARCHIVED_PAGE,
        false => BOOKMARKS_PAGE,
    };
    let back = format!("{base}{}", wanted.to_query());
    let next = page.next.map(|cursor| {
        format!(
            "{base}{}",
            BookmarkQuery {
                cursor: Some(cursor),
                ..wanted
            }
            .to_query()
        )
    });

    render(BookmarksPage {
        section: "bookmarks",
        archived,
        heading: match archived {
            true => "archived",
            false => "bookmarks",
        },
        folders: data::folder_links(&folders, query.folder.as_deref(), BOOKMARKS_PAGE),
        topics: data::topic_links(&topics, query.topic.as_deref(), BOOKMARKS_PAGE),
        search: query.query.clone().unwrap_or_default(),
        back,
        rows,
        has_next: next.is_some(),
        next: next.unwrap_or_default(),
    })
}

async fn new_bookmark(State(state): State<AppState>) -> Result<Html<String>, Failure> {
    render_new(&state, String::new()).await
}

async fn render_new(state: &AppState, error: String) -> Result<Html<String>, Failure> {
    let folders = data::folders(state.client()).await?;
    let visibility_public = data::publish_bookmarks(state.client()).await?;
    render(NewBookmarkPage {
        section: "bookmarks",
        save_folders: data::choices(&folders, true, None),
        visibility_public,
        has_error: !error.is_empty(),
        error,
    })
}

async fn show(State(state): State<AppState>, Path(id): Path<i64>) -> Result<Html<String>, Failure> {
    let client = state.client();
    let bookmark: Bookmark = client.get_json(&bookmark_path(id)).await?;
    let folders = data::folders(client).await?;
    let topics = bookmark
        .topics
        .iter()
        .map(|topic| topic.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    render(BookmarkPage {
        section: "bookmarks",
        public: bookmark.visibility == Visibility::Public,
        archived: bookmark.archived_at.is_some(),
        has_entry: bookmark.entry_id.is_some(),
        entry_id_text: bookmark
            .entry_id
            .map(|entry| entry.to_string())
            .unwrap_or_default(),
        note: bookmark.note.clone().unwrap_or_default(),
        created: reader::when(bookmark.created_at, now()),
        updated: reader::when(bookmark.updated_at, now()),
        folders: data::choices(&folders, true, bookmark.folder_id),
        bookmark,
        topics,
    })
}

async fn create(State(state): State<AppState>, Form(form): Form<CreateForm>) -> Response {
    let request = BookmarkRequest {
        url: form.url,
        title: Some(form.title).filter(|title| !title.trim().is_empty()),
        note: data::optional_text(form.note),
        folder_id: data::id(&form.folder_id),
        topics: data::topics_of(&form.topics),
        visibility: Some(form.visibility),
    };
    match state.client().post(BOOKMARKS_PATH, &request).await {
        Ok(reply) if reply.is_success() => match reply.json::<Bookmark>() {
            Ok(bookmark) => Redirect::to(&bookmark_page(bookmark.id)).into_response(),
            Err(error) => Failure::from(error).into_response(),
        },
        Ok(reply) => render_new(&state, reason(&reply)).await.into_response(),
        Err(error) => render_new(&state, error.to_string()).await.into_response(),
    }
}

async fn change(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<ChangeForm>,
) -> Response {
    let change = BookmarkChange {
        url: Some(form.url),
        canonical_url: None,
        title: Some(form.title),
        note: Some(data::optional_text(form.note)),
        folder_id: Some(data::id(&form.folder_id)),
        position: Some(form.position.max(0) as usize),
        archived: Some(form.archived.is_some()),
        visibility: Some(form.visibility),
    };
    match state.client().patch(&bookmark_path(id), &change).await {
        Ok(reply) if reply.is_success() => Redirect::to(&bookmark_page(id)).into_response(),
        Ok(reply) => Failure::from_reply(&reply).into_response(),
        Err(error) => Failure::from(error).into_response(),
    }
}

async fn set_topics(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<TopicsForm>,
) -> Response {
    let request = TopicsRequest {
        topics: data::topics_of(&form.topics),
    };
    match state
        .client()
        .put(&bookmark_topics_path(id), &request)
        .await
    {
        Ok(reply) if reply.is_success() => Redirect::to(&bookmark_page(id)).into_response(),
        Ok(reply) => Failure::from_reply(&reply).into_response(),
        Err(error) => Failure::from(error).into_response(),
    }
}

async fn archive(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<ArchiveForm>,
) -> Response {
    let change = BookmarkChange {
        archived: Some(form.on),
        ..BookmarkChange::default()
    };
    match state.client().patch(&bookmark_path(id), &change).await {
        Ok(reply) if reply.is_success() => {
            Redirect::to(&return_to(form.back, BOOKMARKS_PAGE)).into_response()
        }
        Ok(reply) => Failure::from_reply(&reply).into_response(),
        Err(error) => Failure::from(error).into_response(),
    }
}

async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.client().delete(&bookmark_path(id)).await {
        Ok(reply) if reply.is_success() => Redirect::to(BOOKMARKS_PAGE).into_response(),
        Ok(reply) => Failure::from_reply(&reply).into_response(),
        Err(error) => Failure::from(error).into_response(),
    }
}

async fn folders(State(state): State<AppState>) -> Result<Html<String>, Failure> {
    render_folders(&state, String::new()).await
}

async fn render_folders(state: &AppState, error: String) -> Result<Html<String>, Failure> {
    let folders = data::folders(state.client()).await?;
    let rows = data::tree(&folders)
        .into_iter()
        .map(|row| FolderRow {
            id: row.folder.id,
            name: row.folder.name,
            indent: data::tree_prefix(row.depth, row.last),
            kind: row.folder.kind.as_str(),
            position: row.folder.position,
            parent_text: row
                .folder
                .parent_id
                .map(|parent| parent.to_string())
                .unwrap_or_default(),
            topics: row
                .folder
                .topics
                .iter()
                .map(|topic| topic.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            dynamic: row.folder.kind == Kind::Dynamic,
        })
        .collect();

    render(FoldersPage {
        section: "bookmarks",
        rows,
        parents: data::choices(&folders, false, None),
        has_error: !error.is_empty(),
        error,
    })
}

async fn create_folder(State(state): State<AppState>, Form(form): Form<FolderForm>) -> Response {
    let folder = NewFolder {
        parent_id: data::id(&form.parent_id),
        name: form.name,
        kind: form.kind,
        topics: match form.kind {
            Kind::Manual => Vec::new(),
            Kind::Dynamic => data::topics_of(&form.topics),
        },
    };
    match state.client().post(FOLDERS_PATH, &folder).await {
        Ok(reply) if reply.is_success() => Redirect::to(FOLDERS_PAGE).into_response(),
        Ok(reply) => render_folders(&state, reason(&reply)).await.into_response(),
        Err(error) => render_folders(&state, error.to_string())
            .await
            .into_response(),
    }
}

async fn change_folder(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<FolderChangeForm>,
) -> Response {
    let change = FolderChange {
        name: Some(form.name),
        parent_id: Some(data::id(&form.parent_id)),
        position: Some(form.position.max(0) as usize),
        topics: match form.kind {
            Kind::Manual => None,
            Kind::Dynamic => Some(data::topics_of(&form.topics)),
        },
    };
    match state.client().patch(&folder_path(id), &change).await {
        Ok(reply) if reply.is_success() => Redirect::to(FOLDERS_PAGE).into_response(),
        Ok(reply) => render_folders(&state, reason(&reply)).await.into_response(),
        Err(error) => render_folders(&state, error.to_string())
            .await
            .into_response(),
    }
}

async fn delete_folder(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.client().delete(&folder_path(id)).await {
        Ok(reply) if reply.is_success() => Redirect::to(FOLDERS_PAGE).into_response(),
        Ok(reply) => render_folders(&state, reason(&reply)).await.into_response(),
        Err(error) => render_folders(&state, error.to_string())
            .await
            .into_response(),
    }
}

async fn topics(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Html<String>, Failure> {
    render_topics(&state, &query, String::new()).await
}

async fn render_topics(
    state: &AppState,
    query: &ListQuery,
    error: String,
) -> Result<Html<String>, Failure> {
    let page: Page<Topic> = state
        .client()
        .get_json(&format!("{TOPICS_PATH}{}", query.to_query()))
        .await?;
    let next = page.next.map(|cursor| {
        format!(
            "{TOPICS_PAGE}{}",
            ListQuery {
                limit: None,
                cursor: Some(cursor)
            }
            .to_query()
        )
    });

    render(TopicsPage {
        section: "bookmarks",
        topics: page
            .items
            .into_iter()
            .map(|topic| TopicRow {
                id: topic.id,
                name: topic.name.clone(),
                href: format!(
                    "{BOOKMARKS_PAGE}{}",
                    BookmarkQuery {
                        topic: Some(topic.name.clone()),
                        ..BookmarkQuery::default()
                    }
                    .to_query()
                ),
                followed: topic.followed_at.is_some(),
            })
            .collect(),
        has_next: next.is_some(),
        next: next.unwrap_or_default(),
        has_error: !error.is_empty(),
        error,
    })
}

async fn rename_topic(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<TopicForm>,
) -> Response {
    let change = TopicChange {
        name: Some(form.name),
    };
    match state.client().patch(&topic_path(id), &change).await {
        Ok(reply) if reply.is_success() => Redirect::to(TOPICS_PAGE).into_response(),
        Ok(reply) => render_topics(&state, &ListQuery::default(), reason(&reply))
            .await
            .into_response(),
        Err(error) => render_topics(&state, &ListQuery::default(), error.to_string())
            .await
            .into_response(),
    }
}

async fn delete_topic(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.client().delete(&topic_path(id)).await {
        Ok(reply) if reply.is_success() => Redirect::to(TOPICS_PAGE).into_response(),
        Ok(reply) => render_topics(&state, &ListQuery::default(), reason(&reply))
            .await
            .into_response(),
        Err(error) => render_topics(&state, &ListQuery::default(), error.to_string())
            .await
            .into_response(),
    }
}

fn row(bookmark: Bookmark, now: i64) -> BookmarkRow {
    let (note, has_note) = match bookmark.note {
        Some(note) if !note.trim().is_empty() => {
            (reader::truncate(&note, reader::TEXT_LIMIT), true)
        }
        _ => (String::new(), false),
    };
    BookmarkRow {
        id: bookmark.id,
        title: bookmark.title,
        url: bookmark.url.clone(),
        domain: data::domain(&bookmark.url),
        note,
        has_note,
        topics: data::topic_links(&bookmark.topics, None, BOOKMARKS_PAGE),
        public: bookmark.visibility == Visibility::Public,
        archived: bookmark.archived_at.is_some(),
        stamp: reader::stamp(bookmark.created_at, now),
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateForm {
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    topics: String,
    visibility: Visibility,
}

#[derive(Debug, Deserialize)]
pub struct ChangeForm {
    title: String,
    url: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    folder_id: Option<String>,
    #[serde(default)]
    position: i64,
    visibility: Visibility,
    #[serde(default)]
    archived: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TopicsForm {
    #[serde(default)]
    topics: String,
}

#[derive(Debug, Deserialize)]
pub struct ArchiveForm {
    on: bool,
    #[serde(default)]
    back: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FolderForm {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    kind: Kind,
    #[serde(default)]
    topics: String,
}

#[derive(Debug, Deserialize)]
pub struct FolderChangeForm {
    name: String,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    position: i64,
    kind: Kind,
    #[serde(default)]
    topics: String,
}

#[derive(Debug, Deserialize)]
pub struct TopicForm {
    name: String,
}

#[derive(Template)]
#[template(path = "../templates/bookmarks/list.html")]
pub struct BookmarksPage {
    pub section: &'static str,
    pub archived: bool,
    pub heading: &'static str,
    pub folders: Vec<data::NavLink>,
    pub topics: Vec<data::NavLink>,
    pub search: String,
    /// Page which the row buttons return to.
    pub back: String,
    pub rows: Vec<BookmarkRow>,
    pub has_next: bool,
    pub next: String,
}

/// One saved link in the list.
pub struct BookmarkRow {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub domain: String,
    pub note: String,
    pub has_note: bool,
    pub topics: Vec<data::NavLink>,
    pub public: bool,
    pub archived: bool,
    /// How long ago the link was saved.
    pub stamp: String,
}

#[derive(Template)]
#[template(path = "../templates/bookmarks/new.html")]
pub struct NewBookmarkPage {
    pub section: &'static str,
    pub save_folders: Vec<data::Choice>,
    pub visibility_public: bool,
    pub has_error: bool,
    pub error: String,
}

/// One topic in the topic page.
pub struct TopicRow {
    pub id: i64,
    pub name: String,
    /// The saved links which carry it.
    pub href: String,
    pub followed: bool,
}

#[derive(Template)]
#[template(path = "../templates/bookmarks/bookmark.html")]
pub struct BookmarkPage {
    pub section: &'static str,
    pub bookmark: Bookmark,
    /// Comma-separated topic names.
    pub topics: String,
    pub folders: Vec<data::Choice>,
    pub has_entry: bool,
    /// Source entry ID, or an empty string.
    pub entry_id_text: String,
    pub note: String,
    pub public: bool,
    pub archived: bool,
    /// How long ago the link was saved.
    pub created: String,
    /// How long ago the link last changed.
    pub updated: String,
}

#[derive(Template)]
#[template(path = "../templates/bookmarks/folders.html")]
pub struct FoldersPage {
    pub section: &'static str,
    pub rows: Vec<FolderRow>,
    /// Every folder, for the parent selectors.
    pub parents: Vec<data::Choice>,
    pub has_error: bool,
    pub error: String,
}

/// One folder in the folder page.
pub struct FolderRow {
    pub id: i64,
    pub name: String,
    /// Tree branch prefix.
    pub indent: String,
    pub kind: &'static str,
    pub position: i64,
    /// Parent ID, or an empty string for a root folder.
    pub parent_text: String,
    pub topics: String,
    pub dynamic: bool,
}

#[derive(Template)]
#[template(path = "../templates/bookmarks/topics.html")]
pub struct TopicsPage {
    pub section: &'static str,
    pub topics: Vec<TopicRow>,
    pub has_next: bool,
    pub next: String,
    pub has_error: bool,
    pub error: String,
}
