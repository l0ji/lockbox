//! Fixtures shared by this crate's tests.

#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Temporary directory for one test, removed when it goes out of scope.
pub struct Temp(PathBuf);

impl Temp {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("lockbox-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temporary directory");
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Make `dir` read-only and report on whether writes are refused.
#[cfg(unix)]
pub fn lock(dir: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o500)).expect("lock directory");
    let probe = dir.join(".probe");
    let locked = fs::write(&probe, "").is_err();
    let _ = fs::remove_file(&probe);
    locked
}

/// Unlock `dir` and give it full permissions again.
#[cfg(unix)]
pub fn unlock(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).expect("unlock directory");
}
