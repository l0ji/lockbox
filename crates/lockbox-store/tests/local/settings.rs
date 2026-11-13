//! Tests for [`lockbox_store::local::settings`].

use std::collections::HashMap;

use crate::common::Temp;
use crate::fixtures::open;
use lockbox_store::{Db, local::settings};

/// Read the settings table.
async fn read(db: &Db) -> HashMap<String, String> {
    db.call(|connection| settings::load(connection))
        .await
        .expect("read settings")
}

/// Modify the settings table.
async fn write(db: &Db, stored: HashMap<String, String>) {
    db.transaction(move |transaction| settings::store(transaction, &stored))
        .await
        .expect("write settings")
}

#[tokio::test]
async fn starts_empty() {
    let temp = Temp::new("settings");
    let db = open(&temp);

    assert!(read(&db).await.is_empty());
    db.close().await.expect("close database");
}

#[tokio::test]
async fn replaces_what_was_stored() {
    let temp = Temp::new("changed-settings");
    let db = open(&temp);

    let first = HashMap::from([
        ("publish_bookmarks".to_owned(), "0".to_owned()),
        ("trust_depth".to_owned(), "3".to_owned()),
    ]);
    write(&db, first.clone()).await;
    assert_eq!(read(&db).await, first);

    // The table holds exactly what the last write had
    let second = HashMap::from([("trust_depth".to_owned(), "4".to_owned())]);
    write(&db, second.clone()).await;
    assert_eq!(read(&db).await, second);
    db.close().await.expect("close database");
}
