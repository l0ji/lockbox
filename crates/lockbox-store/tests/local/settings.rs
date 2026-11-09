//! Tests for [`lockbox_store::local::settings`].

use crate::common::Temp;
use crate::fixtures::open;
use lockbox_store::{
    Error,
    local::settings::{self, Settings},
};

#[tokio::test]
async fn starts_from_the_defaults() {
    let temp = Temp::new("defaults");
    let db = open(&temp);

    let settings = settings::read(&db).await.expect("read settings");
    assert_eq!(settings, Settings::default());
    assert!(settings.publish_bookmarks);
    assert_eq!(settings.trust_depth, 2);
    assert_eq!(settings.retention_days, None);
    assert_eq!(settings.unknown_retention_days, 180);
    assert_eq!(settings.unknown_cap_bytes, 8_388_608);
    assert_eq!(settings.unknown_total_bytes, 536_870_912);
    assert_eq!(settings.rank_quality_weight, 0.6);
    assert_eq!(settings.rank_topic_weight, 0.4);
    assert_eq!(settings.rank_agree_weight, 0.05);
    assert_eq!(settings.rank_half_life_hours, 48);
    assert_eq!(settings.rank_decay_exponent, 1.8);
    assert_eq!(settings.tombstone_days, 365);
    assert_eq!(settings.site_refresh_seconds, 3600);
    assert_eq!(settings.entry_retention_days, 90);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn stores_only_what_differs_from_the_defaults() {
    let temp = Temp::new("changed-settings");
    let db = open(&temp);

    let changed = Settings {
        publish_bookmarks: false,
        trust_depth: 3,
        retention_days: Some(30),
        ..Settings::default()
    };
    settings::write(&db, &changed)
        .await
        .expect("write settings");
    assert_eq!(settings::read(&db).await.expect("read settings"), changed);

    let rows: Vec<(String, String)> = db
        .call(|connection| {
            let mut statement =
                connection.prepare("SELECT key, value FROM setting ORDER BY key")?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
        .expect("read the settings table");
    assert_eq!(
        rows,
        [
            ("publish_bookmarks".to_owned(), "0".to_owned()),
            ("retention_days".to_owned(), "30".to_owned()),
            ("trust_depth".to_owned(), "3".to_owned()),
        ]
    );

    // Back at the defaults, the rows go away again
    settings::write(&db, &Settings::default())
        .await
        .expect("write settings");
    let stored: i64 = db
        .call(|connection| {
            Ok(connection.query_row("SELECT count(*) FROM setting", [], |row| row.get(0))?)
        })
        .await
        .expect("count settings");
    assert_eq!(stored, 0);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn refuses_a_setting_it_cannot_read() {
    let temp = Temp::new("broken-setting");
    let db = open(&temp);
    db.call(|connection| {
        connection.execute(
            "INSERT INTO setting (key, value) VALUES ('trust_depth', 'many')",
            [],
        )?;
        Ok(())
    })
    .await
    .expect("store a broken setting");

    let error = settings::read(&db)
        .await
        .expect_err("the value does not parse");
    assert!(matches!(error, Error::Invalid(_)), "{error}");
    assert!(error.to_string().contains("trust_depth"), "{error}");
    db.close().await.expect("close database");
}
