//! Fixtures shared by the crate's integration tests.

// Each test file links its own copy, so not every fixture is used.
#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

use axum::{Json, Router, routing::get};
use lockbox_proto::api::{STATUS_PATH, Status};

/// A sample status reply from the daemon.
pub fn sample_status() -> Status {
    Status {
        protocol_version: 1,
        daemon_version: "0.1.0".to_owned(),
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
