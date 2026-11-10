//! Fixtures shared by the crate's integration tests.

// Each test file links its own copy, so not every fixture is used.
#![allow(dead_code)]

use std::{
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use lockbox_core::{Node, SecretKey};
use lockbox_store::{Db, local};
use lockboxd::{AppState, serve};
use serde::de::DeserializeOwned;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
    task::JoinHandle,
};

/// A throwaway directory which is removed when the test ends.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lockboxd-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    /// The path of the directory itself.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside the directory, whose parents may not exist yet.
    pub fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A mock daemon serving one node and a local database on a socket in a
/// temporary directory.
pub struct Daemon {
    _dir: TempDir,
    socket: PathBuf,
    db: Arc<Db>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<io::Result<()>>>,
}

impl Daemon {
    pub async fn start() -> Self {
        let dir = TempDir::new();
        let socket = dir.join("daemon.sock");
        let node = Arc::new(
            Node::bind(SecretKey::generate())
                .await
                .expect("node must bind"),
        );
        let db = Arc::new(local::open(&dir.join("local.db")).expect("open database"));

        let (stop, stopped) = oneshot::channel();
        let served = socket.clone();
        let state = AppState::new(node, Arc::clone(&db));
        let task = tokio::spawn(async move {
            serve(&served, state, async {
                let _ = stopped.await;
            })
            .await
        });
        for _ in 0..200 {
            if socket.try_exists().unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(
            socket.try_exists().expect("check socket"),
            "daemon must bind its socket"
        );

        Self {
            _dir: dir,
            socket,
            db,
            stop: Some(stop),
            task: Some(task),
        }
    }

    /// Socket the daemon listens on.
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The daemon's local database.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// Stop the daemon and wait for it to remove its socket.
    pub async fn stop(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.await
                .expect("serve task must not panic")
                .expect("serve must stop cleanly");
        }
        assert!(
            !self.socket.try_exists().expect("check socket"),
            "socket must be removed on shutdown"
        );
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Send a request to the daemon's socket.
///
/// Returns the response status and its body.
pub async fn request(socket: &Path, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("connect to daemon");
    let body = body.unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: lockboxd\r\nConnection: close\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.expect("read reply");
    let (head, body) = reply.split_once("\r\n\r\n").expect("reply has a body");
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("reply has a status code");
    (status, body.to_owned())
}

/// Parse a reply body as `T`.
pub fn json<T: DeserializeOwned>(body: &str) -> T {
    match serde_json::from_str(body) {
        Ok(value) => value,
        Err(err) => panic!("cannot read {body}: {err}"),
    }
}
