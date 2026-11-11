//! Tests of the API's routes, cursors and query strings.

use lockbox_proto::api::{
    ApiError, Code, Cursor, DEFAULT_LIMIT, ListQuery, Page,
    cursor::Key,
    node, reader,
    reader::{DEFAULT_USER_AGENT, EntryQuery},
};
use serde_json::json;

#[test]
fn round_trips_a_cursor() {
    let cursor = Cursor {
        sort: "entries".to_owned(),
        key: vec![Key::Int(1_700_000_000), Key::Text("blog post".to_owned())],
        id: 42,
    };

    let text = cursor.encode().expect("encode cursor");
    assert_eq!(
        Cursor::decode(&text, "entries").expect("decode cursor"),
        cursor
    );
}

#[test]
fn writes_a_cursor_as_url_safe_text() {
    let cursor = Cursor {
        sort: "bookmarks/newest".to_owned(),
        key: vec![Key::Text("https://example.com/?".to_owned())],
        id: i64::MAX,
    };

    let text = cursor.encode().expect("encode cursor");
    assert!(!text.contains('='), "{text}");
    assert!(
        text.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
        "{text}"
    );
}

#[test]
fn refuses_a_cursor_made_for_another_sort() {
    let text = Cursor {
        sort: "entries".to_owned(),
        key: vec![Key::Int(1)],
        id: 1,
    }
    .encode()
    .expect("encode cursor");

    let error = Cursor::decode(&text, "bookmarks").expect_err("another sort must be refused");
    assert_eq!(error.code, Code::Invalid);
    assert!(error.error.contains("entries"), "{error:?}");
}

#[test]
fn refuses_text_which_is_not_a_cursor() {
    for text in ["", "not base64url!", "e30", "MA"] {
        let error = Cursor::decode(text, "entries").expect_err("garbage must be refused");
        assert_eq!(error.code, Code::Invalid, "{text}");
    }
}

#[test]
fn writes_a_page_with_its_next_cursor() {
    let page = Page {
        items: vec![1, 2],
        next: Some("abc".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(&page).expect("page json"),
        json!({"items": [1, 2], "next": "abc"})
    );

    let end: Page<u8> = Page {
        items: Vec::new(),
        next: None,
    };
    assert_eq!(
        serde_json::to_value(&end).expect("page json"),
        json!({"items": [], "next": null})
    );
}

#[test]
fn writes_an_error_with_its_code() {
    let error = ApiError::invalid("cursor is not base64url");

    assert_eq!(
        serde_json::to_value(&error).expect("error json"),
        json!({"code": "invalid", "error": "cursor is not base64url"})
    );
    assert_eq!(ApiError::internal("boom").code, Code::Internal);
}

#[test]
fn names_the_node_routes() {
    assert_eq!(node::HEALTH_PATH, "/api/node/health");
    assert_eq!(node::STATUS_PATH, "/api/node/status");
    assert_eq!(node::SETTINGS_PATH, "/api/node/settings");
}

#[test]
fn names_the_reader_routes() {
    assert_eq!(reader::FEEDS_PATH, "/api/reader/feeds");
    assert_eq!(reader::feed_path(7), "/api/reader/feeds/7");
    assert_eq!(reader::refresh_path(7), "/api/reader/feeds/7/refresh");
    assert_eq!(reader::ENTRIES_PATH, "/api/reader/entries");
    assert_eq!(reader::entry_path(7), "/api/reader/entries/7");
}

#[test]
fn writes_a_list_query() {
    let empty = ListQuery::default();
    assert_eq!(empty.to_query(), "");

    let paged = ListQuery {
        limit: Some(DEFAULT_LIMIT),
        cursor: Some("a-b_c".to_owned()),
    };
    assert_eq!(paged.to_query(), "?limit=50&cursor=a-b_c");
}

#[test]
fn writes_a_reading_list_query() {
    let query = EntryQuery {
        feed: Some(3),
        read: Some(false),
        starred: Some(true),
        query: Some("two words & more".to_owned()),
        limit: Some(5),
        ..EntryQuery::default()
    };

    assert_eq!(
        query.to_query(),
        "?feed=3&read=false&starred=true&query=two%20words%20%26%20more&limit=5"
    );
}

#[test]
fn reads_a_reading_list_query() {
    let query: EntryQuery =
        serde_urlencoded::from_str("feed=3&read=false&query=hello+world&limit=5")
            .expect("read query");

    assert_eq!(query.feed, Some(3));
    assert_eq!(query.read, Some(false));
    assert_eq!(query.starred, None);
    assert_eq!(query.query.as_deref(), Some("hello world"));
    assert_eq!(query.limit, Some(5));
}

#[test]
fn turns_a_reading_list_query_into_a_filter() {
    let filter = EntryQuery {
        feed: Some(3),
        read: Some(false),
        archived: Some(true),
        query: Some("hello".to_owned()),
        limit: Some(5),
        cursor: Some("abc".to_owned()),
        ..EntryQuery::default()
    }
    .filter();

    assert_eq!(filter.feed_id, Some(3));
    assert_eq!(filter.read, Some(false));
    assert_eq!(filter.starred, None);
    assert_eq!(filter.archived, Some(true));
    assert_eq!(filter.query.as_deref(), Some("hello"));
}

#[test]
fn names_the_default_user_agent() {
    assert!(
        DEFAULT_USER_AGENT.starts_with("lockbox/"),
        "{DEFAULT_USER_AGENT}"
    );
    assert!(
        DEFAULT_USER_AGENT.ends_with(env!("CARGO_PKG_VERSION")),
        "{DEFAULT_USER_AGENT}"
    );
}
