//! Common code for building the HTTP API for clients to use, handles the
//! creation of the socket's containing folder and file, and loads or generates
//! the account keypair.

pub mod api;
pub mod reader;
pub mod refresh;

use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};

use lockbox_core::{
    BoxError, Node, SecretKey,
    fetch::Fetcher,
    key::{self, ReadError},
};
use lockbox_proto::key as encoding;
use lockbox_store::Db;
use tokio::net::UnixListener;

/// State shared by the daemon's routes.
#[derive(Clone)]
pub struct AppState {
    node: Arc<Node>,
    db: Arc<Db>,
    fetcher: Fetcher,
}

impl AppState {
    /// Build state around a live node, the local database and an HTTP client.
    pub fn new(node: Arc<Node>, db: Arc<Db>, fetcher: Fetcher) -> Self {
        Self { node, db, fetcher }
    }

    /// The node behind the API.
    pub fn node(&self) -> &Arc<Node> {
        &self.node
    }

    /// The local database behind the API.
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The client which fetches the documents the reader asks for.
    pub fn fetcher(&self) -> &Fetcher {
        &self.fetcher
    }
}

/// Serve the local API on the UNIX socket at `socket` until `shutdown` resolves.
///
/// This function automatically replaces a socket file left behind by a dead
/// daemon.
pub async fn serve(
    socket: &Path,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let listener = bind(socket)?;
    tracing::info!(socket = %socket.display(), "daemon listening");

    let served = axum::serve(listener, api::router(state))
        .with_graceful_shutdown(shutdown)
        .await;
    let _ = fs::remove_file(socket);
    tracing::info!(socket = %socket.display(), "daemon stopped");
    served
}

/// Bind a UNIX listener on `socket`.
pub fn bind(socket: &Path) -> io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        create_private_dir(dir)?;
    }
    clear_stale_socket(socket)?;

    let listener = std::os::unix::net::UnixListener::bind(socket)?;
    listener.set_nonblocking(true)?;
    // Keeps the socket file itself owner-only
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))?;
    UnixListener::from_std(listener)
}

/// Creates a private directory for itself to store the socket.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    if dir.as_os_str().is_empty() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Clears out a socket that no process is utilizing.
///
/// If the path is live, it returns an `AddrInUse` error instead.
fn clear_stale_socket(socket: &Path) -> io::Result<()> {
    if !socket.try_exists()? {
        return Ok(());
    }
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!(
                "active lockboxd instance already running on {}",
                socket.display()
            ),
        )),
        Err(_) => {
            tracing::warn!(socket = %socket.display(), "removing stale socket");
            fs::remove_file(socket)
        }
    }
}

/// The account keypair the daemon runs as.
pub struct Keypair {
    /// Secret key this node authenticates with.
    pub secret: SecretKey,
    /// File the secret key was read from, or written to.
    pub path: PathBuf,
    /// Whether the keypair was generated on this run.
    pub created: bool,
}

impl Keypair {
    /// Public key of this account, encoded with the `lbox` prefix.
    pub fn public(&self) -> String {
        encoding::encode_public(self.secret.public().as_bytes())
    }

    /// Short form of the public key, for comparing.
    pub fn fingerprint(&self) -> String {
        encoding::fingerprint(self.secret.public().as_bytes())
    }
}

/// Load the account keypair from `dir`, generating one when there is none.
///
/// A corrupt key file is an error rather than a reason to generate another one,
/// which would silently move the account to a new identity.
pub fn load_keypair(dir: &Path) -> Result<Keypair, BoxError> {
    let path = dir.join(key::SECRET_FILE);
    match key::read_secret(dir) {
        Ok(bytes) => {
            let keypair = Keypair {
                secret: SecretKey::from_bytes(&bytes),
                path,
                created: false,
            };
            replace_public_file(dir, &keypair);
            Ok(keypair)
        }
        Err(ReadError::Missing) => create_keypair(dir),
        Err(err) => Err(format!("cannot read {}: {err}", path.display()).into()),
    }
}

/// Generate an account keypair and store both of its files in `dir`.
fn create_keypair(dir: &Path) -> Result<Keypair, BoxError> {
    let secret = SecretKey::generate();
    let path = dir.join(key::SECRET_FILE);
    key::write_secret(dir, &secret.to_bytes())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))?;
    let public = dir.join(key::PUBLIC_FILE);
    key::write_public(dir, secret.public().as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", public.display()))?;
    Ok(Keypair {
        secret,
        path,
        created: true,
    })
}

/// Write the public key file back when only the secret one still exists.
fn replace_public_file(dir: &Path, keypair: &Keypair) {
    let file = dir.join(key::PUBLIC_FILE);
    match file.try_exists() {
        Ok(true) => return,
        Ok(false) => {}
        Err(err) => {
            tracing::warn!(file = %file.display(), "cannot check the public key: {err}");
            return;
        }
    }
    let public = keypair.secret.public();
    if let Err(err) = key::write_public(dir, public.as_bytes()) {
        tracing::warn!(file = %file.display(), "cannot write the public key: {err}");
    }
}
