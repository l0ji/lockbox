//! Routes meant to be used for a common C2S API.

/// Route that returns [`lockbox_types::node::Health`].
pub const HEALTH_PATH: &str = "/api/node/health";

/// Route that returns [`lockbox_types::node::Status`].
pub const STATUS_PATH: &str = "/api/node/status";

/// Route which reads and changes the node's settings.
pub const SETTINGS_PATH: &str = "/api/node/settings";
