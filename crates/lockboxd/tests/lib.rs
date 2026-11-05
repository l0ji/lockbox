//! Tests of the UNIX socket handling and the local API.

use std::{
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use lockbox_core::{Node, SecretKey};
use lockbox_proto::{
    api::{HEALTH_PATH, STATUS_PATH},
    key::{self, PUBLIC_FILE, PUBLIC_PREFIX, SECRET_FILE},
};
use lockboxd::{AppState, bind, load_keypair, serve};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};

/// A throwaway directory which is removed when the test ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lockboxd-test-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    /// The directory itself.
    fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside the directory, whose parents may not exist yet.
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One `GET` over the socket which is read and then subsequently closes the
/// connection.
async fn get(socket: &Path, path: &str) -> String {
    let mut stream = tokio::net::UnixStream::connect(socket)
        .await
        .expect("connect to daemon");
    let request = format!("GET {path} HTTP/1.1\r\nHost: lockboxd\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .await
        .expect("write request");

    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.expect("read reply");
    reply
}

/// Permssion mode bits of `path`.
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path)
        .expect("stat path")
        .permissions()
        .mode()
        & 0o777
}

#[tokio::test]
async fn binds_a_private_socket() {
    let dir = TempDir::new();
    let socket = dir.join("nest/lockbox/daemon.sock");

    let listener = bind(&socket).expect("bind must succeed");
    assert!(socket.exists(), "socket file must exist");
    assert_eq!(mode(&socket), 0o600, "socket must be owner-only");
    assert_eq!(
        mode(socket.parent().expect("socket has a directory")),
        0o700,
        "socket directory must be private"
    );

    drop(listener);
}

#[tokio::test]
async fn replaces_a_socket_left_by_a_dead_daemon() {
    let dir = TempDir::new();
    let socket = dir.join("daemon.sock");

    let dead = std::os::unix::net::UnixListener::bind(&socket).expect("bind stale socket");
    drop(dead);
    assert!(socket.exists(), "stale socket file must remain");

    let listener = bind(&socket).expect("bind must replace the stale socket");
    assert!(
        std::fs::metadata(&socket)
            .expect("stat socket")
            .file_type()
            .is_socket(),
        "socket must be rebound"
    );

    drop(listener);
}

#[tokio::test]
async fn refuses_a_socket_that_is_in_use() {
    let dir = TempDir::new();
    let socket = dir.join("daemon.sock");
    let active = std::os::unix::net::UnixListener::bind(&socket).expect("bind active socket");

    let error = bind(&socket).expect_err("a live socket must not be replaced");
    assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
    assert!(
        error
            .to_string()
            .contains("active lockboxd instance already running"),
        "{error}"
    );

    drop(active);
}

#[tokio::test]
async fn serves_the_local_api() {
    let dir = TempDir::new();
    let socket = dir.join("daemon.sock");
    let node = Arc::new(
        Node::bind(SecretKey::generate())
            .await
            .expect("node must bind"),
    );
    let node_id = node.id().to_string();

    let (stop, stopped) = oneshot::channel();
    let served = socket.clone();
    let task = tokio::spawn(async move {
        serve(&served, AppState::new(node), async {
            let _ = stopped.await;
        })
        .await
    });

    for _ in 0..200 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(socket.exists(), "daemon must bind its socket");

    let reply = get(&socket, HEALTH_PATH).await;
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(reply.contains(r#""ok":true"#), "{reply}");

    let reply = get(&socket, STATUS_PATH).await;
    assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
    assert!(
        reply.contains(&format!(r#""node_id":"{node_id}""#)),
        "{reply}"
    );
    assert!(reply.contains(r#""protocol_version":1"#), "{reply}");

    stop.send(()).ok();
    task.await
        .expect("serve task must not panic")
        .expect("serve must stop cleanly");
    assert!(!socket.exists(), "socket must be removed on shutdown");
}

#[test]
fn generates_an_account_keypair_when_there_is_none() {
    let dir = TempDir::new();
    let keypair = load_keypair(dir.path()).expect("keypair");

    assert!(keypair.created);
    assert!(keypair.public().starts_with("lbox0"));
    let public = keypair.public();
    let fingerprint = keypair.fingerprint().replace('-', "");
    let data = &public[PUBLIC_PREFIX.len() + 1..];
    assert_eq!(&data[..fingerprint.len()], fingerprint);
    assert!(dir.join(SECRET_FILE).is_file());
    let filed = std::fs::read_to_string(dir.join(PUBLIC_FILE)).expect("public key file");
    assert_eq!(filed.trim(), keypair.public());
    assert_eq!(
        std::fs::read_to_string(dir.join(SECRET_FILE))
            .expect("secret key file")
            .trim(),
        key::encode_secret(&keypair.secret.to_bytes())
    );
}

#[test]
fn loads_the_account_keypair_it_generated() {
    let dir = TempDir::new();
    let first = load_keypair(dir.path()).expect("first keypair");
    let second = load_keypair(dir.path()).expect("second keypair");

    assert!(first.created);
    assert!(!second.created);
    assert_eq!(first.public(), second.public());
    assert_eq!(first.secret.to_bytes(), second.secret.to_bytes());
}

#[test]
fn refuses_a_corrupt_secret_file() {
    let dir = TempDir::new();
    std::fs::write(dir.join(SECRET_FILE), "not a key\n").expect("write corrupt secret");

    let err = match load_keypair(dir.path()) {
        Ok(_) => panic!("a corrupt secret must not be accepted"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("secret.key"), "{err}");
    assert!(!dir.join(PUBLIC_FILE).exists());
}

#[test]
fn restores_a_missing_public_key_file() {
    let dir = TempDir::new();
    let first = load_keypair(dir.path()).expect("first keypair");
    std::fs::remove_file(dir.join(PUBLIC_FILE)).expect("remove public key file");

    let second = load_keypair(dir.path()).expect("second keypair");
    assert_eq!(first.public(), second.public());
    let filed = std::fs::read_to_string(dir.join(PUBLIC_FILE)).expect("public key file");
    assert_eq!(filed.trim(), second.public());
}
