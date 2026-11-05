//! Common code for building the HTTP API for clients to use and handles the
//! creation of the socket's containing folder and file.

use std::{
    fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
    sync::Arc,
};

use axum::{Json, Router, extract::State, routing::get};
use lockbox_core::Node;
use lockbox_proto::api::{HEALTH_PATH, Health, STATUS_PATH, Status};
use tokio::net::UnixListener;

/// State shared by the daemon's routes.
#[derive(Clone)]
pub struct AppState {
    node: Arc<Node>,
}

impl AppState {
    /// Build state around a live node.
    pub fn new(node: Arc<Node>) -> Self {
        Self { node }
    }

    /// The node behind the API.
    pub fn node(&self) -> &Arc<Node> {
        &self.node
    }
}

/// Build the router that serves the local API.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(STATUS_PATH, get(status))
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health { ok: true })
}

async fn status(State(state): State<AppState>) -> Json<Status> {
    Json(state.node.status())
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

    let served = axum::serve(listener, router(state))
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
    if !socket.exists() {
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
