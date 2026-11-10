//! Tests of the feed parser and of discovering the feed in pages which
//! advertise a feed.

mod common;

use lockbox_core::feed;
use lockbox_proto::api::reader::Kind;
use url::Url;

/// A small RSS document with the fields a reader shows.
const RSS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Example blog</title>
    <link>https://example.com/</link>
    <description>Words</description>
    <item>
      <title>First post</title>
      <link>https://example.com/first</link>
      <guid>first</guid>
      <description>Short summary</description>
      <pubDate>Sun, 06 Sep 2009 16:20:00 +0000</pubDate>
    </item>
    <item>
      <link>https://example.com/second</link>
      <guid>second</guid>
    </item>
  </channel>
</rss>"#;

/// The same feed as Atom, which parses to the same model.
const ATOM: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Example blog</title>
  <link href="https://example.com/"/>
  <entry>
    <title>First post</title>
    <link href="https://example.com/first"/>
    <id>first</id>
    <updated>2009-09-06T16:20:00Z</updated>
    <author><name>Jane Doe</name></author>
    <content type="html">&lt;p&gt;words&lt;/p&gt;</content>
  </entry>
</feed>"#;

/// A page which advertises a feed under a relative address.
const PAGE: &str = r#"<!doctype html>
<html>
  <head>
    <title>A page</title>
    <link rel="stylesheet" href="/style.css">
    <link rel="alternate" type="application/rss+xml" href="/feed.xml">
  </head>
  <body>words</body>
</html>"#;

/// A base address which relative addresses resolve against.
fn base() -> Url {
    Url::parse("https://example.com/blog/").expect("base url")
}

#[test]
fn reads_an_rss_document() {
    let parsed = feed::parse(RSS.as_bytes(), None).expect("parse rss");

    assert_eq!(parsed.kind, Kind::Rss);
    assert_eq!(parsed.title.as_deref(), Some("Example blog"));
    assert_eq!(parsed.site_url.as_deref(), Some("https://example.com/"));

    let first = &parsed.entries[0];
    assert_eq!(first.source, "first");
    assert_eq!(first.title, "First post");
    assert_eq!(first.url.as_deref(), Some("https://example.com/first"));
    assert_eq!(first.summary.as_deref(), Some("Short summary"));
    assert_eq!(first.published_at, Some(1_252_254_000));

    // An item without a title is named after the page it points at
    let second = &parsed.entries[1];
    assert_eq!(second.title, "https://example.com/second");
    assert_eq!(second.published_at, None);
}

#[test]
fn reads_an_atom_document() {
    let parsed = feed::parse(ATOM.as_bytes(), None).expect("parse atom");

    assert_eq!(parsed.kind, Kind::Atom);
    assert_eq!(parsed.site_url.as_deref(), Some("https://example.com/"));
    let entry = &parsed.entries[0];
    assert_eq!(entry.source, "first");
    assert_eq!(entry.author.as_deref(), Some("Jane Doe"));
    assert_eq!(entry.body.as_deref(), Some("<p>words</p>"));
    assert_eq!(entry.published_at, Some(1_252_254_000));
}

#[test]
fn reads_a_json_feed() {
    let document = r#"{
        "version": "https://jsonfeed.org/version/1.1",
        "title": "Example blog",
        "home_page_url": "https://example.com/",
        "items": [{"id": "first", "url": "https://example.com/first", "title": "First post"}]
    }"#;
    let parsed = feed::parse(document.as_bytes(), None).expect("parse json");

    assert_eq!(parsed.kind, Kind::Json);
    assert_eq!(parsed.title.as_deref(), Some("Example blog"));
    assert_eq!(parsed.entries[0].source, "first");
}

#[test]
fn resolves_relative_addresses_against_the_document() {
    let document = r#"<rss version="2.0"><channel><title>Example</title>
        <item><title>A post</title><link>/first</link><guid>first</guid></item>
        </channel></rss>"#;
    let parsed = feed::parse(document.as_bytes(), Some(&base())).expect("parse rss");

    assert_eq!(
        parsed.entries[0].url.as_deref(),
        Some("https://example.com/first")
    );
}

#[test]
fn refuses_a_document_which_is_not_a_feed() {
    let error =
        feed::parse(b"<html><body>words</body></html>", None).expect_err("a page is not a feed");
    assert!(!error.to_string().is_empty());
}

#[test]
fn discovers_the_feed_which_a_page_advertises() {
    let url = feed::discover(PAGE, &base()).expect("the page advertises a feed");
    assert_eq!(url.as_str(), "https://example.com/feed.xml");
}

#[test]
fn discovers_nothing_when_a_page_advertises_nothing() {
    let page = r#"<html><head><link rel="stylesheet" href="/style.css"></head></html>"#;
    assert_eq!(feed::discover(page, &base()), None);
    assert_eq!(feed::discover("", &base()), None);
}

#[test]
fn reads_the_attributes_a_page_may_quote_differently() {
    for page in [
        r#"<link rel='alternate' type='application/atom+xml' href='feed.xml'>"#,
        r#"<LINK REL=alternate TYPE="application/atom+xml" HREF=feed.xml>"#,
        r#"<link href="feed.xml" type="application/feed+json" rel="alternate feed">"#,
    ] {
        let url = feed::discover(page, &base()).unwrap_or_else(|| panic!("{page}"));
        assert_eq!(
            url.as_str(),
            "https://example.com/blog/feed.xml",
            "a relative address resolves against the page"
        );
    }
}
