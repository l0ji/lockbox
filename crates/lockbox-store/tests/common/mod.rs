//! Helpers shared by the storage tests.

#![allow(dead_code)]

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
};

/// A throwaway directory which is removed when the test ends.
pub struct Temp(PathBuf);

impl Temp {
    /// Create a directory for one test.
    pub fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lockbox-store-{name}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    /// Path of a database file inside the directory.
    pub fn db(&self) -> PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
