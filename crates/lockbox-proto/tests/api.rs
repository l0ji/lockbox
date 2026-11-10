//! Tests of the API types and the cursor type scheme.

use lockbox_proto::api::{ApiError, Code, Cursor, Page, cursor::Key, node};
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
}
