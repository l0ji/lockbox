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
