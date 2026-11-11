//! The clock which every row and every reply is stamped with.

use std::time::{SystemTime, UNIX_EPOCH};

/// Time in seconds since the UNIX epoch.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
