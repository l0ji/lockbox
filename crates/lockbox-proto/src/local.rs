//! Definition of how the UNIX socket is opened.
//!
//! The server which runs a node binds this socket path and any client connects
//! to it so that everything in the repository should resolve it with
//! [`socket_path`], unless the user passes an explicit location.

use std::path::PathBuf;

/// Environment variable that overrides the socket path for all binaries.
pub const SOCKET_ENV: &str = "LOCKBOX_SOCKET";

/// Path to the UNIX socket that a node listens on.
///
/// [`SOCKET_ENV`] wins when it is set and non-empty. Otherwise it is set to
/// [`default_socket_path`].
pub fn socket_path() -> PathBuf {
    match std::env::var_os(SOCKET_ENV) {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => default_socket_path(),
    }
}

/// Default (per-user) socket path which is used when [`SOCKET_ENV`] is not set.
///
/// Prefers `$XDG_RUNTIME_DIR`, then `$TMPDIR`, then `/tmp`, and always nests
/// the socket in a `lockbox` directory to prevent any conflicts.
pub fn default_socket_path() -> PathBuf {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("lockbox").join("daemon.sock")
}
