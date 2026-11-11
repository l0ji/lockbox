//! Tests of the status page template.

mod common;

use askama::Template;
use common::sample_status;
use lockbox_proto::{PROTOCOL_VERSION, api::node::Status};
use lockbox_web::{
    EntriesPage, EntryPage, ErrorPage, FeedLink, FeedRow, FeedsPage, FilterLink, Page, Row,
    TEXT_LIMIT, truncate,
};

/// Render a page, which isn't expected to fail with the embedded page.
fn render(page: Page) -> String {
    page.render().expect("render page")
}

/// Sample status with an uptime given by `secs`.
fn with_uptime(secs: u64) -> Status {
    let mut status = sample_status();
    status.uptime_secs = secs;
    status
}

#[test]
fn renders_a_live_node() {
    let html = render(Page::live(sample_status()));

    assert!(html.contains(r#"data-state="live""#), "{html}");
    assert!(html.contains("<th>Public key</th>"), "{html}");
    assert!(
        html.contains("<td>lbox1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0ssqs23x</td>"),
        "{html}"
    );
    assert!(html.contains("<th>Hex</th>"), "{html}");
    assert!(html.contains("<td>test-node</td>"), "{html}");
    assert!(
        html.contains(&format!("<td>{PROTOCOL_VERSION}</td>")),
        "{html}"
    );
    assert!(html.contains("<td>0.1.0</td>"), "{html}");
    assert!(html.contains("<td>42s</td>"), "{html}");
    assert!(html.contains("https://relay.example./"), "{html}");
    assert!(html.contains("127.0.0.1:5555"), "{html}");
}

#[test]
fn links_its_stylesheet() {
    let html = render(Page::live(sample_status()));

    assert!(
        html.contains(r#"<link rel="stylesheet" href="/style.css">"#),
        "{html}"
    );
    assert!(!html.contains("<style>"), "{html}");
}

#[test]
fn renders_empty_lists_as_none() {
    let status = Status {
        relay_urls: Vec::new(),
        direct_addrs: Vec::new(),
        ..sample_status()
    };

    let html = render(Page::live(status));
    assert_eq!(html.matches(r#"<li class="empty">none</li>"#).count(), 2);
}

#[test]
fn renders_a_down_page() {
    let html = render(Page::down("the daemon is offline: no such file"));

    assert!(html.contains(r#"data-state="down""#), "{html}");
    assert!(html.contains("daemon offline"), "{html}");
    assert!(
        html.contains("the daemon is offline: no such file"),
        "{html}"
    );
}

#[test]
fn formats_uptime_in_two_units() {
    for (secs, text) in [
        (0, "0s"),
        (59, "59s"),
        (61, "1m 1s"),
        (3600, "1h 0m"),
        (90000, "1d 1h"),
    ] {
        let html = render(Page::live(with_uptime(secs)));
        assert!(html.contains(&format!("<td>{text}</td>")), "{secs}: {html}");
    }
}

/// Render any page, which isn't expected to fail with the embedded templates.
fn rendered<T: Template>(page: T) -> String {
    page.render().expect("render page")
}

#[test]
fn renders_the_reading_list() {
    let html = rendered(EntriesPage {
        section: "reader",
        filters: vec![
            FilterLink {
                label: "all",
                href: "/".to_owned(),
                selected: false,
            },
            FilterLink {
                label: "unread",
                href: "/?read=false".to_owned(),
                selected: true,
            },
        ],
        feeds: vec![
            FeedLink {
                title: "all feeds".to_owned(),
                href: "/".to_owned(),
                selected: false,
            },
            FeedLink {
                title: "Example blog".to_owned(),
                href: "/?feed=1".to_owned(),
                selected: true,
            },
        ],
        search: "words".to_owned(),
        back: "/?read=false".to_owned(),
        rows: vec![
            Row {
                link: "/entry/1".to_owned(),
                title: "First post".to_owned(),
                feed: "Example blog".to_owned(),
                stamp: "3h".to_owned(),
                excerpt: "Short summary".to_owned(),
                has_excerpt: true,
                read: false,
                starred: true,
                read_next: true,
                starred_next: false,
                id: 1,
            },
            Row {
                link: "/entry/2".to_owned(),
                title: "Second post".to_owned(),
                feed: "Example blog".to_owned(),
                stamp: "2d".to_owned(),
                excerpt: String::new(),
                has_excerpt: false,
                read: true,
                starred: false,
                read_next: false,
                starred_next: true,
                id: 2,
            },
        ],
        has_next: true,
        next: "/?cursor=abc".to_owned(),
    });

    assert!(html.contains(r#"href="/entry/1""#), "{html}");
    assert!(html.contains("First post"), "{html}");
    assert!(
        html.contains(r#"<span class="feed">Example blog</span>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<div class="stamp starred">3h</div>"#),
        "{html}"
    );
    // The row carries its own two buttons, and they answer to this page
    assert!(
        html.contains(r#"<form class="act read" method="post""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<form class="act star undo" method="post""#),
        "the starred row offers to undo it: {html}"
    );
    assert!(
        html.contains(r#"<form class="act read undo" method="post""#),
        "the read row offers to undo it: {html}"
    );
    assert!(
        html.contains(r#"action="/entry/1/read""#) && html.contains(r#"action="/entry/1/star""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="hidden" name="back" value="/?read=false">"#),
        "{html}"
    );
    assert!(html.contains(">read</button>"), "{html}");
    assert!(html.contains(">unstar</button>"), "{html}");
    assert!(html.contains(">unread</button>"), "{html}");
    assert!(html.contains(">star</button>"), "{html}");
    assert!(html.contains(r#"<li class="item read">"#), "{html}");
    // The filter and the feed which are in use are marked
    assert!(
        html.contains(r#"<a class="filter on" href="/?read=false">unread</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<li class="on"><a href="/?feed=1">Example blog</a></li>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<a href="/?cursor=abc">older entries</a>"#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="search" name="query" value="words""#),
        "{html}"
    );
}

#[test]
fn renders_an_empty_reading_list() {
    let html = rendered(EntriesPage {
        section: "reader",
        filters: Vec::new(),
        feeds: Vec::new(),
        search: String::new(),
        back: "/".to_owned(),
        rows: Vec::new(),
        has_next: false,
        next: String::new(),
    });

    assert!(html.contains("nothing to read"), "{html}");
    assert!(html.contains("none yet"), "{html}");
    assert!(!html.contains("older entries"), "{html}");
}

#[test]
fn renders_one_entry() {
    let html = rendered(EntryPage {
        section: "reader",
        title: "First post".to_owned(),
        feed: "Example blog".to_owned(),
        author: "Penny".to_owned(),
        has_author: true,
        link: "https://example.com/first".to_owned(),
        has_url: true,
        when: "3h ago".to_owned(),
        excerpt: "Short summary".to_owned(),
        has_excerpt: true,
        body: "<p>words</p>".to_owned(),
        has_body: true,
        back: "/".to_owned(),
    });

    assert!(
        html.contains("<title>lockbox: First post</title>"),
        "{html}"
    );
    assert!(html.contains("Example blog | 3h ago | Penny"), "{html}");
    assert!(
        !html.contains("mark read"),
        "the actions live in the list: {html}"
    );
    assert!(!html.contains(r#"name="on""#), "{html}");
    assert!(
        !html.contains("<p>words</p>"),
        "the body is escaped: {html}"
    );
    assert!(!html.contains("Short summary"), "the body wins: {html}");
}

#[test]
fn renders_an_entry_without_a_body() {
    let html = rendered(EntryPage {
        section: "reader",
        title: "Second post".to_owned(),
        feed: "Example blog".to_owned(),
        author: String::new(),
        has_author: false,
        link: String::new(),
        has_url: false,
        when: "2d ago".to_owned(),
        excerpt: "Short summary".to_owned(),
        has_excerpt: true,
        body: String::new(),
        has_body: false,
        back: "/".to_owned(),
    });

    assert!(
        html.contains("Short summary"),
        "the summary stands in: {html}"
    );
    assert!(!html.contains("mark unread"), "{html}");
    assert!(!html.contains(r#"name="on""#), "{html}");
    assert!(!html.contains("Penny"), "{html}");
}

#[test]
fn renders_the_feed_list() {
    let html = rendered(FeedsPage {
        section: "feeds",
        feeds: vec![
            FeedRow {
                id: 1,
                title: "Example blog".to_owned(),
                url: "https://example.com/feed.xml".to_owned(),
                kind: "rss",
                interval: "1h".to_owned(),
                polled: "3h ago".to_owned(),
                timeout: 20,
                agent: "Penny's reader".to_owned(),
                paused: false,
                has_error: false,
                error: String::new(),
            },
            FeedRow {
                id: 2,
                title: "Broken feed".to_owned(),
                url: "https://broken.example/feed".to_owned(),
                kind: "atom",
                interval: "10m".to_owned(),
                polled: "never".to_owned(),
                timeout: 90,
                agent: String::new(),
                paused: true,
                has_error: true,
                error: "cannot fetch https://broken.example/feed".to_owned(),
            },
        ],
        agent: "lockbox/0.1.0".to_owned(),
        has_error: true,
        error: "https://example.com is neither a feed nor a page with one".to_owned(),
    });

    assert!(
        html.contains("rss | https://example.com/feed.xml | polled 3h ago"),
        "{html}"
    );
    assert!(html.contains("<td>1h</td>"), "{html}");
    assert!(
        html.contains("atom | https://broken.example/feed | polled never"),
        "{html}"
    );
    assert!(html.contains("<td>10m</td>"), "{html}");
    assert!(html.contains("paused"), "{html}");
    assert!(
        html.contains("cannot fetch https://broken.example/feed"),
        "{html}"
    );
    assert!(
        html.contains("neither a feed nor a page with one"),
        "{html}"
    );
    assert!(html.contains(r#"action="/feeds/2/poll""#), "{html}");
    assert!(
        html.contains(r#"<form class="add" method="post" action="/feeds">"#),
        "{html}"
    );
    // Each feed carries the settings its fetches use, and the default as a hint
    assert!(
        html.contains(r#"<input type="number" name="timeout_seconds" value="20""#),
        "{html}"
    );
    assert!(
        html.contains(r#"<input type="text" name="user_agent" value="Penny&#39;s reader""#),
        "{html}"
    );
    assert!(html.contains(r#"placeholder="lockbox/0.1.0""#), "{html}");
    assert!(html.contains(r#"action="/feeds/1/settings""#), "{html}");
}

#[test]
fn renders_an_error_page() {
    let html = rendered(ErrorPage {
        section: "",
        status: 502,
        message: "daemon unexpectedly replied 500".to_owned(),
        offline: false,
    });

    assert!(html.contains("<title>lockbox: 502</title>"), "{html}");
    assert!(html.contains("daemon unexpectedly replied 500"), "{html}");
    assert!(
        html.contains(r#"<a href="/">back to the reader</a>"#),
        "{html}"
    );
}

#[test]
fn renders_an_offline_page() {
    let html = rendered(ErrorPage {
        section: "",
        status: 502,
        message: "the daemon is offline".to_owned(),
        offline: true,
    });

    assert!(html.contains("<title>lockbox: 502</title>"), "{html}");
    assert!(html.contains(r#"<h1 class="section">502</h1>"#), "{html}");
    assert!(html.contains("the daemon is offline"), "{html}");
    assert!(
        html.contains("Start the daemon and reload this page."),
        "{html}"
    );
    assert!(!html.contains("daemon.sock"), "no path in the page: {html}");
    assert!(!html.contains("back to the reader"), "{html}");
}

#[test]
fn cuts_a_body_which_is_too_long() {
    let body = "word ".repeat(100);
    let html = rendered(EntryPage {
        section: "reader",
        title: "A long post".to_owned(),
        feed: "Example blog".to_owned(),
        author: String::new(),
        has_author: false,
        link: String::new(),
        has_url: false,
        when: "2d ago".to_owned(),
        excerpt: String::new(),
        has_excerpt: false,
        body: truncate(&body, TEXT_LIMIT),
        has_body: true,
        back: "/".to_owned(),
    });

    let cut = body.chars().take(TEXT_LIMIT).collect::<String>();
    assert!(html.contains(&cut), "{html}");
    assert!(html.contains("..."), "{html}");
    assert!(!html.contains(&body), "the rest of the body is cut: {html}");
}

#[test]
fn keeps_a_body_which_fits() {
    let body = "short enough";
    let html = rendered(EntryPage {
        section: "reader",
        title: "A short post".to_owned(),
        feed: "Example blog".to_owned(),
        author: String::new(),
        has_author: false,
        link: String::new(),
        has_url: false,
        when: "2d ago".to_owned(),
        excerpt: String::new(),
        has_excerpt: false,
        body: truncate(body, TEXT_LIMIT),
        has_body: true,
        back: "/".to_owned(),
    });

    assert!(html.contains(body), "{html}");
    assert!(!html.contains("..."), "{html}");
}
