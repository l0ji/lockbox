//! Refreshing the feeds the reader follows when their next refresh is due.

use std::{sync::Arc, time::Duration};

use lockbox_core::fetch::Fetcher;
use lockbox_store::{Db, local::feeds};
use lockbox_types::now;

use crate::reader;

/// How often the daemon looks for a feed which is due.
pub const TICK: Duration = Duration::from_secs(30);

/// How many due feeds one pass takes.
const BATCH: usize = 32;

/// Refresh the feeds which are due, every `tick`, until the task is stopped.
pub async fn run(db: Arc<Db>, fetcher: Fetcher, tick: Duration) {
    loop {
        tokio::time::sleep(tick).await;
        refresh_due(&db, &fetcher).await;
    }
}

/// Refresh one batch of due feeds, one at a time.
async fn refresh_due(db: &Db, fetcher: &Fetcher) {
    let due = match feeds::due(db, now(), BATCH).await {
        Ok(due) => due,
        Err(error) => {
            tracing::error!("cannot list the feeds which are due: {error}");
            return;
        }
    };
    for feed in due {
        match reader::refresh(db, fetcher, feed).await {
            Ok(refreshed) => tracing::info!(
                url = %refreshed.feed.url,
                new_entries = refreshed.new_entries,
                "refreshed feed"
            ),
            // The failure is in the feed's row, which the reader shows
            Err(error) => tracing::warn!("{error}"),
        }
    }
}
