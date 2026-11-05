//! Tests of the status page template.

mod common;

use askama::Template;
use common::sample_status;
use lockbox_proto::api::Status;
use lockbox_web::Page;

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
    assert!(html.contains("<dd>test-node</dd>"), "{html}");
    assert!(html.contains("<dd>1</dd>"), "{html}");
    assert!(html.contains("<dd>0.1.0</dd>"), "{html}");
    assert!(html.contains("<dd>42s</dd>"), "{html}");
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
    assert_eq!(html.matches(r#"<li class="none">none</li>"#).count(), 2);
}

#[test]
fn renders_a_down_page() {
    let html = render(Page::down("failed to reach daemon: no such file"));

    assert!(html.contains(r#"data-state="down""#), "{html}");
    assert!(html.contains("daemon unreachable"), "{html}");
    assert!(
        html.contains("failed to reach daemon: no such file"),
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
        assert!(html.contains(&format!("<dd>{text}</dd>")), "{secs}: {html}");
    }
}
