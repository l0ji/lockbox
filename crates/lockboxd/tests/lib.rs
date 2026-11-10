//! Tests of the UNIX socket handling and the account keypair.

mod common;

use std::{
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::Path,
};

use common::{Daemon, TempDir};
use lockbox_proto::key::{self, PUBLIC_FILE, PUBLIC_PREFIX, SECRET_FILE};
use lockboxd::{bind, load_keypair};

/// Make `dir` read-only and report on whether writes are refused.
fn lock(dir: &Path) -> bool {
    let mut permissions = std::fs::metadata(dir).expect("metadata").permissions();
    permissions.set_mode(0o500);
    std::fs::set_permissions(dir, permissions).expect("lock directory");

    let probe = dir.join(".probe");
    let locked = std::fs::write(&probe, "").is_err();
    let _ = std::fs::remove_file(&probe);
    locked
}

/// Unlock `dir` and give it full permissions again.
fn unlock(dir: &Path) {
    let mut permissions = std::fs::metadata(dir).expect("metadata").permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(dir, permissions).expect("unlock directory");
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
    assert!(
        socket.try_exists().expect("check socket"),
        "socket file must exist"
    );
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
    assert!(
        socket.try_exists().expect("check socket"),
        "stale socket file must remain"
    );

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
async fn removes_its_socket_when_stopped() {
    let daemon = Daemon::start().await;
    assert!(
        daemon.socket().try_exists().expect("check socket"),
        "socket must exist while serving"
    );
    daemon.stop().await;
}

#[test]
fn generates_an_account_keypair_when_there_is_none() {
    let dir = TempDir::new();
    let keypair = load_keypair(dir.path()).expect("keypair");

    assert!(keypair.created);
    assert!(keypair.public().starts_with("lbox1"));
    let public = keypair.public();
    let fingerprint = keypair.fingerprint().replace('-', "");
    let data = &public[PUBLIC_PREFIX.len() + 1..];
    assert_eq!(&data[..fingerprint.len()], fingerprint);

    assert!(
        dir.join(SECRET_FILE)
            .try_exists()
            .expect("check secret file")
    );
    let filed = std::fs::read_to_string(dir.join(PUBLIC_FILE)).expect("public key file");
    assert_eq!(filed.trim(), public);
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
    assert!(
        !dir.join(PUBLIC_FILE)
            .try_exists()
            .expect("check public file")
    );
}

#[test]
fn restores_a_missing_public_key_file() {
    let dir = TempDir::new();
    let first = load_keypair(dir.path()).expect("first keypair");
    std::fs::remove_file(dir.join(PUBLIC_FILE)).expect("remove public key file");

    let second = load_keypair(dir.path()).expect("second keypair");
    assert_eq!(first.public(), second.public());
    let public = second.public();
    let filed = std::fs::read_to_string(dir.join(PUBLIC_FILE)).expect("public key file");
    assert_eq!(filed.trim(), public);
}

#[test]
fn refuses_to_generate_a_keypair_it_cannot_write() {
    let dir = TempDir::new();
    if !lock(dir.path()) {
        unlock(dir.path());
        return;
    }

    let result = load_keypair(dir.path());
    unlock(dir.path());

    let err = match result {
        Ok(_) => panic!("a locked directory must not be given a new keypair"),
        Err(err) => err,
    };
    assert!(err.to_string().contains("secret.key"), "{err}");
    assert!(
        !dir.join(SECRET_FILE)
            .try_exists()
            .expect("check secret file")
    );
    assert!(
        !dir.join(PUBLIC_FILE)
            .try_exists()
            .expect("check public file")
    );
}
