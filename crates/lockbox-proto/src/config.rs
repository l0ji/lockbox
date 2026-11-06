//! Where lockbox keeps its files.
//!
//! These are the names that a downstream crate would use when storing data.

/// Environment variable that overrides the directory for all files.
pub const DIR_ENV: &str = "LOCKBOX_DIR";

/// Directory that an administrator may install files in.
pub const SYSTEM_DIR: &str = "/etc/lockbox";

/// Directory under `$XDG_CONFIG_HOME` that holds the files.
pub const CONFIG_DIR: &str = "lockbox";

/// Directory under `$HOME` that holds the files when the config one is locked.
pub const HOME_DIR: &str = ".lockbox";
