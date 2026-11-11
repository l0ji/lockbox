//! The routes for the RSS reader's pages.

use std::collections::HashMap;

use askama::Template;
use axum::{
    Form, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use lockbox_proto::api::{
    ApiError, ListQuery, Page,
    reader::{
        DEFAULT_USER_AGENT, ENTRIES_PATH, EntryQuery, FEEDS_PATH, entry_path, feed_path,
        refresh_path,
    },
};
use lockbox_types::{
    entry::{Entry, EntryChange, EntrySummary},
    feed::{Feed, FeedChange, NewFeed},
    now,
};
use serde::Deserialize;

use crate::{AppState, ClientError, DaemonClient, ErrorPage, Response as Reply};

/// Page which lists the reader's entries.
pub const ENTRIES_PAGE: &str = "/";

/// Base of the page which shows a single entry.
pub const ENTRY_PAGE: &str = "/entry";

/// Characters of text which a page shows before it truncates.
pub const TEXT_LIMIT: usize = 300;

/// Page which lists the subscribed feeds, with a form to add one.
pub const FEEDS_PAGE: &str = "/feeds";

/// Address of one entry's page.
pub fn entry_page(id: i64) -> String {
    format!("{ENTRY_PAGE}/{id}")
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(ENTRIES_PAGE, get(entries))
        .route(&format!("{ENTRY_PAGE}/{{id}}"), get(entry))
        .route(&format!("{ENTRY_PAGE}/{{id}}/read"), post(mark_read))
        .route(&format!("{ENTRY_PAGE}/{{id}}/star"), post(mark_starred))
        .route(FEEDS_PAGE, get(feeds).post(add))
        .route(&format!("{FEEDS_PAGE}/{{id}}/refresh"), post(refresh))
        .route(&format!("{FEEDS_PAGE}/{{id}}/settings"), post(settings))
        .route(&format!("{FEEDS_PAGE}/{{id}}/delete"), post(remove))
}

/// Filter and paging of the reading list, as a browser sends them.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PageQuery {
    /// Only entries from this feed.
    feed: Option<i64>,
    /// Only show read entries when true.
    read: Option<bool>,
    /// Only show starred entries when true.
    starred: Option<bool>,
    /// Words to search the entries for.
    query: Option<String>,
    /// Cursor which the previous page ended with.
    cursor: Option<String>,
}

impl PageQuery {
    fn to_entry_query(&self) -> EntryQuery {
        EntryQuery {
            feed: self.feed,
            read: self.read,
            starred: self.starred,
            query: self.query.clone(),
            cursor: self.cursor.clone(),
            ..EntryQuery::default()
        }
    }
}

/// Show the reading list.
async fn entries(
    State(state): State<AppState>,
    Query(query): Query<PageQuery>,
) -> Result<Html<String>, Failure> {
    let client = state.client();
    let feeds = all_feeds(client).await?;
    let page: Page<EntrySummary> = client
        .get_json(&format!(
            "{ENTRIES_PATH}{}",
            query.to_entry_query().to_query()
        ))
        .await?;

    let names: HashMap<i64, String> = feeds
        .iter()
        .map(|feed| (feed.id, feed_title(feed)))
        .collect();
    let now = now();
    let rows = page
        .items
        .into_iter()
        .map(|entry| row(entry, &names, now))
        .collect();

    let next = page.next.map(|cursor| {
        format!(
            "{ENTRIES_PAGE}{}",
            EntryQuery {
                cursor: Some(cursor),
                ..query.to_entry_query()
            }
            .to_query()
        )
    });

    render(EntriesPage {
        section: "reader",
        filters: filters(&query),
        feeds: feed_links(&feeds, &query),
        search: query.query.clone().unwrap_or_default(),
        back: format!("{ENTRIES_PAGE}{}", query.to_entry_query().to_query()),
        rows,
        has_next: next.is_some(),
        next: next.unwrap_or_default(),
    })
}

/// Show one entry with its full body.
async fn entry(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Html<String>, Failure> {
    let client = state.client();
    let entry: Entry = client.get_json(&entry_path(id)).await?;
    let feeds = all_feeds(client).await?;
    let feed = feeds
        .iter()
        .find(|feed| feed.id == entry.summary.feed_id)
        .map(feed_title)
        .unwrap_or_else(|| "unknown feed".to_owned());

    let summary = entry.summary;
    let (author, has_author) = present(summary.author.clone());
    let (link, has_url) = present(summary.url.clone());
    let (body, has_body) = truncated_text(entry.body);
    let (excerpt, has_excerpt) = truncated_text(summary.summary.clone());

    render(EntryPage {
        section: "reader",
        title: summary.title.clone(),
        feed,
        author,
        has_author,
        link,
        has_url,
        when: when(summary.sort_at, now()),
        excerpt,
        has_excerpt,
        body,
        has_body,
        back: ENTRIES_PAGE.to_owned(),
    })
}

/// Show the feeds which the reader follows.
async fn feeds(State(state): State<AppState>) -> Result<Html<String>, Failure> {
    render_feeds(&state, String::new()).await
}

/// Add a feed which the user names by its address.
async fn add(State(state): State<AppState>, Form(form): Form<AddFeed>) -> Response {
    let request = NewFeed::new(form.url);
    match state.client().post(FEEDS_PATH, &request).await {
        Ok(reply) if reply.is_success() => Redirect::to(FEEDS_PAGE).into_response(),
        Ok(reply) => render_feeds(&state, reason(&reply)).await.into_response(),
        Err(error) => render_feeds(&state, error.to_string())
            .await
            .into_response(),
    }
}

/// Fetch one feed now.
async fn refresh(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.client().post(&refresh_path(id), &()).await {
        Ok(reply) if reply.is_success() => Redirect::to(FEEDS_PAGE).into_response(),
        Ok(reply) => render_feeds(&state, reason(&reply)).await.into_response(),
        Err(error) => render_feeds(&state, error.to_string())
            .await
            .into_response(),
    }
}

/// Change how a feed is fetched.
async fn settings(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<FetchSettings>,
) -> Response {
    let change = FeedChange {
        timeout_seconds: Some(form.timeout_seconds),
        user_agent: Some(user_agent_of(&form.user_agent)),
        ..FeedChange::default()
    };
    match state.client().patch(&feed_path(id), &change).await {
        Ok(reply) if reply.is_success() => Redirect::to(FEEDS_PAGE).into_response(),
        Ok(reply) => render_feeds(&state, reason(&reply)).await.into_response(),
        Err(error) => render_feeds(&state, error.to_string())
            .await
            .into_response(),
    }
}

/// Remove one feed and the entries it brought.
async fn remove(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    match state.client().delete(&feed_path(id)).await {
        Ok(reply) if reply.is_success() => Redirect::to(FEEDS_PAGE).into_response(),
        Ok(reply) => render_feeds(&state, reason(&reply)).await.into_response(),
        Err(error) => render_feeds(&state, error.to_string())
            .await
            .into_response(),
    }
}

/// Mark one entry as read or unread.
async fn mark_read(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<Toggle>,
) -> Response {
    mark(
        &state,
        id,
        EntryChange {
            read: Some(form.on),
            ..EntryChange::default()
        },
        form.back,
    )
    .await
}

/// Star or unstar one entry.
async fn mark_starred(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<Toggle>,
) -> Response {
    mark(
        &state,
        id,
        EntryChange {
            starred: Some(form.on),
            ..EntryChange::default()
        },
        form.back,
    )
    .await
}

/// Change one entry and return to the page the form came from.
async fn mark(state: &AppState, id: i64, change: EntryChange, back: Option<String>) -> Response {
    match state.client().patch(&entry_path(id), &change).await {
        Ok(reply) if reply.is_success() => Redirect::to(&return_to(back)).into_response(),
        Ok(reply) => Failure::from_reply(&reply).into_response(),
        Err(error) => Failure::from(error).into_response(),
    }
}

/// Render the feed page, with the reason a form failed if one did.
async fn render_feeds(state: &AppState, error: String) -> Result<Html<String>, Failure> {
    let feeds = all_feeds(state.client()).await?;
    let now = now();
    let rows = feeds.iter().map(|feed| feed_row(feed, now)).collect();
    render(FeedsPage {
        section: "feeds",
        feeds: rows,
        user_agent: DEFAULT_USER_AGENT.to_owned(),
        has_error: !error.is_empty(),
        error,
    })
}

/// Every feed, following the list's cursors.
async fn all_feeds(client: &DaemonClient) -> Result<Vec<Feed>, ClientError> {
    let mut feeds = Vec::new();
    let mut cursor = None;
    loop {
        let query = ListQuery {
            limit: None,
            cursor: cursor.clone(),
        };
        let page: Page<Feed> = client
            .get_json(&format!("{FEEDS_PATH}{}", query.to_query()))
            .await?;
        feeds.extend(page.items);
        match page.next {
            Some(next) => cursor = Some(next),
            None => return Ok(feeds),
        }
    }
}

/// The filter links which a [`PageQuery`] offers.
fn filters(query: &PageQuery) -> Vec<FilterLink> {
    let current = match (query.read, query.starred) {
        (Some(false), _) => "unread",
        (_, Some(true)) => "starred",
        _ => "all",
    };
    [
        ("all", None, None),
        ("unread", Some(false), None),
        ("starred", None, Some(true)),
    ]
    .into_iter()
    .map(|(label, read, starred)| FilterLink {
        label,
        selected: current == label,
        href: format!(
            "{ENTRIES_PAGE}{}",
            EntryQuery {
                feed: query.feed,
                read,
                starred,
                query: query.query.clone(),
                ..EntryQuery::default()
            }
            .to_query()
        ),
    })
    .collect()
}

/// The sidebar's feed links, which keep the current filters.
fn feed_links(feeds: &[Feed], query: &PageQuery) -> Vec<FeedLink> {
    let link = |title: String, feed: Option<i64>, selected: bool| FeedLink {
        title,
        selected,
        href: format!(
            "{ENTRIES_PAGE}{}",
            EntryQuery {
                feed,
                read: query.read,
                starred: query.starred,
                query: query.query.clone(),
                ..EntryQuery::default()
            }
            .to_query()
        ),
    };

    let mut links = vec![link("all feeds".to_owned(), None, query.feed.is_none())];
    links.extend(
        feeds
            .iter()
            .map(|feed| link(feed_title(feed), Some(feed.id), query.feed == Some(feed.id))),
    );
    links
}

/// One entry as the reading list shows it, linking to the entry's URL or to
/// its own page when it has none.
fn row(entry: EntrySummary, feeds: &HashMap<i64, String>, now: i64) -> Row {
    let (excerpt, has_excerpt) = truncated_text(entry.summary);
    Row {
        link: entry.url.clone().unwrap_or_else(|| entry_page(entry.id)),
        title: entry.title,
        feed: feeds
            .get(&entry.feed_id)
            .cloned()
            .unwrap_or_else(|| "unknown feed".to_owned()),
        stamp: stamp(entry.sort_at, now),
        excerpt,
        has_excerpt,
        read: entry.read_at.is_some(),
        starred: entry.starred_at.is_some(),
        read_next: entry.read_at.is_none(),
        starred_next: entry.starred_at.is_none(),
        id: entry.id,
    }
}

/// One feed as the feeds page shows it.
fn feed_row(feed: &Feed, now: i64) -> FeedRow {
    FeedRow {
        id: feed.id,
        title: feed_title(feed),
        url: feed.url.clone(),
        kind: feed.kind.as_str(),
        interval: interval(feed.refresh_seconds),
        timeout: feed.timeout_seconds,
        user_agent: feed.user_agent.clone().unwrap_or_default(),
        refreshed: match feed.fetched_at {
            Some(at) => when(at, now),
            None => "never".to_owned(),
        },
        paused: feed.disabled,
        has_error: feed.last_error.is_some(),
        error: feed.last_error.clone().unwrap_or_default(),
    }
}

/// Name of a feed, falling back to its address.
fn feed_title(feed: &Feed) -> String {
    match feed.name().trim().is_empty() {
        true => feed.url.clone(),
        false => feed.name().to_owned(),
    }
}

/// How often a feed is refreshed, in the largest unit that fits.
fn interval(seconds: i64) -> String {
    match seconds {
        seconds if seconds % 3600 == 0 => format!("{}h", seconds / 3600),
        seconds if seconds % 60 == 0 => format!("{}m", seconds / 60),
        seconds => format!("{seconds}s"),
    }
}

/// How long ago a timestamp was, in its largest whole unit.
fn age(at: i64, now: i64) -> (i64, &'static str) {
    let seconds = now.saturating_sub(at);
    let days = seconds / 86_400;
    if seconds < 60 {
        (seconds, "s")
    } else if seconds < 3_600 {
        (seconds / 60, "m")
    } else if seconds < 86_400 {
        (seconds / 3_600, "h")
    } else if days < 7 {
        (days, "d")
    } else if days < 365 {
        (days / 7, "w")
    } else {
        (days / 365, "y")
    }
}

/// How long ago a moment was, in words.
fn when(at: i64, now: i64) -> String {
    match age(at, now) {
        (_, "s") => "just now".to_owned(),
        (count, unit) => format!("{count}{unit} ago"),
    }
}

/// How long ago a moment was, as a short stamp.
fn stamp(at: i64, now: i64) -> String {
    match age(at, now) {
        (_, "s") => "now".to_owned(),
        (count, unit) => format!("{count}{unit}"),
    }
}

/// Page a form returns to, which must stay on this site.
fn return_to(back: Option<String>) -> String {
    match back {
        Some(back) if back.starts_with('/') && !back.starts_with("//") => back,
        _ => ENTRIES_PAGE.to_owned(),
    }
}

/// User agent a form names, where an empty one means the default.
fn user_agent_of(user_agent: &str) -> Option<String> {
    let user_agent = user_agent.trim();
    match user_agent.is_empty() {
        true => None,
        false => Some(user_agent.to_owned()),
    }
}

/// The first `limit` characters of `text`, with an ellipsis where it was cut.
pub fn truncate(text: &str, limit: usize) -> String {
    let mut cut: String = text.chars().take(limit).collect();
    if text.chars().nth(limit).is_some() {
        cut.push_str("...");
    }
    cut
}

/// Text cut to [`TEXT_LIMIT`], where an empty string counts as missing.
fn truncated_text(text: Option<String>) -> (String, bool) {
    present(text.map(|text| truncate(&text, TEXT_LIMIT)))
}

/// Text of an optional field, where an empty string counts as missing.
fn present(text: Option<String>) -> (String, bool) {
    match text {
        Some(text) if !text.trim().is_empty() => (text, true),
        _ => (String::new(), false),
    }
}

/// Reason for a failed reply, as given by the daemon.
fn reason(reply: &Reply) -> String {
    match serde_json::from_slice::<ApiError>(&reply.body) {
        Ok(error) => error.error,
        Err(_) => format!("daemon replied {}", reply.status),
    }
}

/// Render a page, mapping a template failure to a server error.
fn render<T: Template>(page: T) -> Result<Html<String>, Failure> {
    match page.render() {
        Ok(body) => Ok(Html(body)),
        Err(error) => {
            tracing::error!("{error}");
            Err(Failure {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: error.to_string(),
                offline: false,
            })
        }
    }
}

/// A failed request, shown by the error page.
#[derive(Debug)]
pub struct Failure {
    status: StatusCode,
    message: String,
    /// Whether the daemon is not running at all.
    offline: bool,
}

impl Failure {
    /// The failure which a reply describes.
    fn from_reply(reply: &Reply) -> Self {
        Self {
            status: StatusCode::from_u16(reply.status).unwrap_or(StatusCode::BAD_GATEWAY),
            message: reason(reply),
            offline: false,
        }
    }
}

impl From<ClientError> for Failure {
    fn from(error: ClientError) -> Self {
        match &error {
            ClientError::Offline => Self {
                status: StatusCode::BAD_GATEWAY,
                message: error.to_string(),
                offline: true,
            },
            ClientError::Status { status, body } => Self {
                status: StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
                message: match serde_json::from_str::<ApiError>(body) {
                    Ok(named) => named.error,
                    Err(_) => error.to_string(),
                },
                offline: false,
            },
            _ => Self {
                status: StatusCode::BAD_GATEWAY,
                message: error.to_string(),
                offline: false,
            },
        }
    }
}

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        if self.status.is_server_error() {
            tracing::warn!(status = %self.status, "{message}", message = self.message);
        }
        let page = ErrorPage {
            section: "",
            status: self.status.as_u16(),
            message: self.message,
            offline: self.offline,
        };
        match page.render() {
            Ok(html) => (self.status, Html(html)).into_response(),
            Err(error) => {
                tracing::error!("{error}");
                (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
            }
        }
    }
}

/// Context for the add feed form.
#[derive(Debug, Deserialize)]
pub struct AddFeed {
    url: String,
}

/// How a feed is fetched, as sent by the settings form.
#[derive(Debug, Deserialize)]
pub struct FetchSettings {
    timeout_seconds: i64,
    user_agent: String,
}

/// The state which a toggle form turns on or off.
#[derive(Debug, Deserialize)]
pub struct Toggle {
    on: bool,
    /// Page to return to, or the reading list when missing.
    back: Option<String>,
}

/// Template context for the reading list.
#[derive(Template)]
#[template(path = "../templates/reader/entries.html")]
pub struct EntriesPage {
    /// Section of the site which the navbar marks.
    pub section: &'static str,
    pub filters: Vec<FilterLink>,
    pub feeds: Vec<FeedLink>,
    /// Search query for a set of entries.
    pub search: String,
    /// Page which the row buttons return to.
    pub back: String,
    /// Feed entries on the page.
    pub rows: Vec<Row>,
    /// Whether there is a page after this one.
    pub has_next: bool,
    /// Address of the next page.
    pub next: String,
}

/// One filter offered by the entries list.
pub struct FilterLink {
    pub label: &'static str,
    pub href: String,
    pub selected: bool,
}

/// A link to one feed's entries.
pub struct FeedLink {
    pub title: String,
    pub href: String,
    pub selected: bool,
}

/// One row of the reading list.
pub struct Row {
    pub link: String,
    pub title: String,
    pub feed: String,
    /// How long ago the entry reached the list.
    pub stamp: String,
    pub has_excerpt: bool,
    pub excerpt: String,
    pub read: bool,
    pub starred: bool,
    /// State which the read button turns the entry into.
    pub read_next: bool,
    /// State which the star button turns the entry into.
    pub starred_next: bool,
    pub id: i64,
}

/// Template context for a single entry in a feed.
#[derive(Template)]
#[template(path = "../templates/reader/entry.html")]
pub struct EntryPage {
    /// Section of the site which the navbar marks.
    pub section: &'static str,
    pub title: String,
    pub feed: String,
    pub has_author: bool,
    pub author: String,
    pub has_url: bool,
    pub link: String,
    pub when: String,
    pub has_excerpt: bool,
    pub excerpt: String,
    pub has_body: bool,
    pub body: String,
    /// Return link to the reading list.
    pub back: String,
}

/// Template context for the feed list.
#[derive(Template)]
#[template(path = "../templates/reader/feeds.html")]
pub struct FeedsPage {
    /// Section of the site which the navbar marks.
    pub section: &'static str,
    pub feeds: Vec<FeedRow>,
    /// User agent a fetch sends when the feed names none.
    pub user_agent: String,
    /// Whether the previous form submission failed.
    pub has_error: bool,
    /// Reason why the form submission failed.
    pub error: String,
}

/// One feed in the feed list.
pub struct FeedRow {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub kind: &'static str,
    pub interval: String,
    pub timeout: i64,
    pub user_agent: String,
    pub refreshed: String,
    pub paused: bool,
    pub has_error: bool,
    pub error: String,
}
