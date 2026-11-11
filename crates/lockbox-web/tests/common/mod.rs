//! Fixtures shared by the crate's integration tests.

// Each test file links its own copy, so not every fixture is used.
#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

use axum::{Json, Router, routing::get};
use lockbox_proto::{
    PROTOCOL_VERSION,
    api::{
        node::{STATUS_PATH, Status},
        reader::{Entry, EntrySummary, Feed, Kind, Visibility},
    },
};

/// A sample status reply from the daemon.
pub fn sample_status() -> Status {
    Status {
        protocol_version: PROTOCOL_VERSION,
        software_version: "0.1.0".to_owned(),
        public_key: "lbox1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0ssqs23x".to_owned(),
        node_id: "test-node".to_owned(),
        relay_urls: vec!["https://relay.example./".to_owned()],
        direct_addrs: vec!["127.0.0.1:5555".to_owned()],
        uptime_secs: 42,
    }
}

/// A stub daemon listening on a socket inside a throwaway directory.
pub struct Stub {
    dir: PathBuf,
    socket: PathBuf,
}

impl Stub {
    /// Serve `status` on the status route.
    pub async fn start(status: Status) -> Self {
        let app = Router::new().route(
            STATUS_PATH,
            get(move || {
                let status = status.clone();
                async move { Json(status) }
            }),
        );
        Self::serve(app).await
    }

    /// Serve a custom router.
    pub async fn serve(app: Router) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lockbox-web-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        let socket = dir.join("daemon.sock");

        let listener = tokio::net::UnixListener::bind(&socket).expect("bind test socket");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { dir, socket }
    }

    /// Socket the stub listens on.
    pub fn socket(&self) -> PathBuf {
        self.socket.clone()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A feed which a stub daemon serves.
pub fn sample_feed() -> Feed {
    Feed {
        id: 1,
        url: "https://example.com/feed.xml".to_owned(),
        site_url: Some("https://example.com/".to_owned()),
        title: Some("Example blog".to_owned()),
        kind: Kind::Rss,
        poll_seconds: 3600,
        timeout_seconds: 20,
        user_agent: None,
        next_poll_at: Some(1_700_003_600),
        fetched_at: Some(1_700_000_000),
        last_error: None,
        visibility: Visibility::Private,
        disabled: false,
        created_at: 1_700_000_000,
        updated_at: 1_700_000_000,
    }
}

/// One entry which a stub daemon serves.
pub fn sample_entry() -> EntrySummary {
    EntrySummary {
        id: 1,
        guid: "abc".to_owned(),
        feed_id: 1,
        title: "First post".to_owned(),
        url: Some("https://example.com/first".to_owned()),
        author: Some("Penny".to_owned()),
        summary: Some("Short summary".to_owned()),
        published_at: Some(1_699_999_000),
        sort_at: 1_700_000_000,
        fetched_at: 1_700_000_000,
        read_at: None,
        starred_at: None,
        archived_at: None,
    }
}

/// The same entry with the body which only the detail route carries.
pub fn sample_body() -> Entry {
    Entry {
        summary: sample_entry(),
        body: Some("<p>words</p>".to_owned()),
        truncated: false,
    }
}
