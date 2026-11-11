//! Tests of the model's text forms and of what it writes to a client.

use lockbox_types::{
    entry::{Entry, EntryChange, EntrySummary},
    feed::{Feed, FeedChange, Kind, Source, Visibility},
};
use serde_json::json;

#[test]
fn names_the_feed_formats() {
    for (kind, name) in [
        (Kind::Rss, "rss"),
        (Kind::Atom, "atom"),
        (Kind::Json, "json"),
    ] {
        assert_eq!(kind.as_str(), name);
        assert_eq!(serde_json::to_value(kind).expect("kind json"), json!(name));
        assert_eq!(Kind::parse(name), Some(kind));
    }
    assert_eq!(Kind::parse("atom-ish"), None);
}

#[test]
fn names_the_visibility_and_the_source() {
    for (visibility, name) in [
        (Visibility::Public, "public"),
        (Visibility::Private, "private"),
    ] {
        assert_eq!(visibility.as_str(), name);
        assert_eq!(
            serde_json::to_value(visibility).expect("visibility json"),
            json!(name)
        );
        assert_eq!(Visibility::parse(name), Some(visibility));
    }

    for (source, name) in [
        (Source::User, "user"),
        (Source::Page, "page"),
        (Source::Federation, "federation"),
    ] {
        assert_eq!(source.as_str(), name);
        assert_eq!(Source::parse(name), Some(source));
    }
}

#[test]
fn writes_an_entry_with_its_body_inline() {
    let entry = Entry {
        summary: EntrySummary {
            id: 1,
            guid: "abc".to_owned(),
            feed_id: 2,
            title: "A post".to_owned(),
            url: None,
            author: None,
            summary: None,
            published_at: None,
            sort_at: 1_700_000_000,
            fetched_at: 1_700_000_000,
            read_at: None,
            starred_at: None,
            archived_at: None,
        },
        body: Some("<p>words</p>".to_owned()),
        truncated: false,
    };

    let json = serde_json::to_value(&entry).expect("entry json");
    assert_eq!(json["title"], "A post");
    assert_eq!(json["guid"], "abc");
    assert_eq!(json["sort_at"], 1_700_000_000);
    assert_eq!(json["body"], "<p>words</p>");
    assert!(json.get("summary_fields").is_none(), "{json}");
}

#[test]
fn names_the_entry_change() {
    assert_eq!(
        serde_json::to_value(EntryChange {
            read: Some(true),
            ..EntryChange::default()
        })
        .expect("change json"),
        json!({"read": true, "starred": null, "archived": null})
    );
}

#[test]
fn reads_a_feed_change_which_clears_the_user_agent() {
    let cleared: FeedChange = serde_json::from_str(r#"{"user_agent":null}"#).expect("change");
    assert_eq!(cleared.user_agent, Some(None));

    let left: FeedChange = serde_json::from_str("{}").expect("change");
    assert_eq!(left.user_agent, None);

    let named: FeedChange =
        serde_json::from_str(r#"{"user_agent":"another reader"}"#).expect("change");
    assert_eq!(named.user_agent, Some(Some("another reader".to_owned())));
    assert_eq!(named.timeout_seconds, None);
}

#[test]
fn leaves_the_node_state_out_of_a_feed() {
    let feed = Feed {
        id: 1,
        url: "https://example.com/feed.xml".to_owned(),
        site_url: Some("https://example.com/".to_owned()),
        title: None,
        kind: Kind::Rss,
        refresh_seconds: 3600,
        timeout_seconds: 20,
        user_agent: None,
        next_refresh_at: None,
        fetched_at: None,
        etag: Some("\"abc\"".to_owned()),
        last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".to_owned()),
        last_error: None,
        visibility: Visibility::Private,
        disabled: false,
        discovered_from: Source::Federation,
        created_at: 10,
        updated_at: 20,
    };

    let json = serde_json::to_value(&feed).expect("feed json");
    assert_eq!(json["url"], "https://example.com/feed.xml");
    assert_eq!(json["kind"], "rss");
    assert_eq!(json["visibility"], "private");
    for name in ["etag", "last_modified", "discovered_from"] {
        assert!(json.get(name).is_none(), "{name} must stay on the node");
    }

    // What a client reads back has none of the node's state
    let read: Feed = serde_json::from_value(json).expect("feed json");
    let without_state = Feed {
        etag: None,
        last_modified: None,
        discovered_from: Source::User,
        ..feed
    };
    assert_eq!(read, without_state);
}
