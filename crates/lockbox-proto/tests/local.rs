//! Tests for the opening of the UNIX socet.

use lockbox_proto::local::default_socket_path;

#[test]
fn default_socket_path_is_nested() {
    let path = default_socket_path();
    assert!(path.ends_with("lockbox/daemon.sock"), "{}", path.display());
}
