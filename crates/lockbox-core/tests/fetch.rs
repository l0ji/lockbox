//! Tests of the HTTP client which the reader fetches documents with.

mod common;

use common::{Reply, Site};
use lockbox_core::fetch::{Fetcher, Request};
use std::time::Duration;

/// A feed document that the stub server responds with as is.
const DOCUMENT: &str = "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel>\
                        <title>Example</title></channel></rss>";

#[tokio::test]
async fn fetches_a_document() {
    let site = Site::start(Reply::document(DOCUMENT).with_etag("\"v1\"")).await;
    let fetcher = Fetcher::new().expect("build the client");

    let fetched = fetcher
        .fetch(&Request {
            url: &site.url("/feed.xml"),
            etag: None,
            last_modified: None,
            timeout: Duration::from_secs(5),
            user_agent: "lockbox-test/1",
        })
        .await
        .expect("fetch the document");

    assert_eq!(fetched.text(), DOCUMENT);
    assert_eq!(fetched.url.as_str(), site.url("/feed.xml"));
    assert_eq!(fetched.etag.as_deref(), Some("\"v1\""));
    assert!(!fetched.unchanged);
    let request = &site.requests()[0];
    assert!(request.starts_with("GET /feed.xml "), "{request}");
    assert!(
        request
            .to_lowercase()
            .contains("user-agent: lockbox-test/1"),
        "{request}"
    );
}

#[tokio::test]
async fn asks_for_a_document_it_already_has() {
    let site = Site::start(Reply::document(DOCUMENT).with_etag("\"v1\"")).await;
    let fetcher = Fetcher::new().expect("build the client");
    let url = site.url("/feed.xml");
    let agent = "lockbox-test/1";
    let first = fetcher
        .fetch(&Request {
            url: &url,
            etag: None,
            last_modified: None,
            timeout: Duration::from_secs(5),
            user_agent: agent,
        })
        .await
        .expect("fetch the document");

    let second = fetcher
        .fetch(&Request {
            url: &url,
            etag: first.etag.as_deref(),
            last_modified: first.last_modified.as_deref(),
            timeout: Duration::from_secs(5),
            user_agent: agent,
        })
        .await
        .expect("fetch the document again");

    assert!(second.unchanged);
    assert!(second.body.is_empty());
    assert_eq!(second.etag.as_deref(), Some("\"v1\""));
    let requests = site.requests();
    assert!(requests[1].to_lowercase().contains("if-none-match: \"v1\""));
}

#[tokio::test]
async fn reports_a_status_which_is_not_a_document() {
    let site = Site::start(Reply {
        status: 404,
        body: "gone".to_owned(),
        etag: None,
    })
    .await;
    let fetcher = Fetcher::new().expect("build the client");

    let error = fetcher
        .fetch(&Request {
            url: &site.url("/feed.xml"),
            etag: None,
            last_modified: None,
            timeout: Duration::from_secs(5),
            user_agent: "lockbox-test/1",
        })
        .await
        .expect_err("a missing document must fail");
    assert!(error.to_string().contains("404"), "{error}");
}

#[tokio::test]
async fn refuses_an_address_which_is_not_a_url() {
    let fetcher = Fetcher::new().expect("build the client");

    let error = fetcher
        .fetch(&Request {
            url: "not a url",
            etag: None,
            last_modified: None,
            timeout: Duration::from_secs(5),
            user_agent: "lockbox-test/1",
        })
        .await
        .expect_err("garbage must be refused");
    assert!(error.to_string().contains("not a URL"), "{error}");
}

#[tokio::test]
async fn gives_up_at_the_deadline_it_was_given() {
    let site = Site::start(Reply::document(DOCUMENT)).await;
    let fetcher = Fetcher::new().expect("build the client");

    let error = fetcher
        .fetch(&Request {
            url: &site.url("/feed.xml"),
            etag: None,
            last_modified: None,
            timeout: Duration::from_millis(1),
            user_agent: "lockbox-test/1",
        })
        .await
        .expect_err("a deadline of a millisecond must pass");

    assert!(error.to_string().contains("timed out"), "{error}");
}
