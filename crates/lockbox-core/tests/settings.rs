//! Tests of [`lockbox_core::settings`].

use std::collections::HashMap;

use lockbox_core::settings::{Settings, SettingsChange};
use serde_json::json;

#[test]
fn writes_every_default_setting() {
    assert_eq!(
        serde_json::to_value(Settings::default()).expect("settings json"),
        json!({
            "publish_bookmarks": true,
            "trust_depth": 2,
            "retention_days": null,
            "unknown_retention_days": 180,
            "unknown_cap_bytes": 8_388_608,
            "unknown_total_bytes": 536_870_912,
            "rank_quality_weight": 0.6,
            "rank_topic_weight": 0.4,
            "rank_agree_weight": 0.05,
            "rank_half_life_hours": 48,
            "rank_decay_exponent": 1.8,
            "tombstone_days": 365,
            "site_refresh_seconds": 3600,
            "entry_retention_days": 90,
        })
    );
}

#[test]
fn fills_in_the_settings_a_client_left_out() {
    let settings: Settings = serde_json::from_str(r#"{"trust_depth": 4}"#).expect("settings json");
    assert_eq!(
        settings,
        Settings {
            trust_depth: 4,
            ..Settings::default()
        }
    );
    assert_eq!(
        serde_json::from_str::<Settings>("{}").expect("settings json"),
        Settings::default()
    );
}

#[test]
fn changes_only_the_settings_it_names() {
    let change: SettingsChange =
        serde_json::from_str(r#"{"trust_depth": 4, "retention_days": null}"#).expect("change json");
    let mut settings = Settings::default();
    settings.change(change);

    assert_eq!(settings.trust_depth, 4);
    assert_eq!(settings.retention_days, None);
    assert_eq!(settings.tombstone_days, Settings::default().tombstone_days);
}

#[test]
fn tells_a_missing_setting_from_a_null_one() {
    let missing: SettingsChange = serde_json::from_str("{}").expect("change json");
    assert_eq!(missing.retention_days, None);
    assert_eq!(missing.trust_depth, None);

    let cleared: SettingsChange =
        serde_json::from_str(r#"{"retention_days": null}"#).expect("change json");
    assert_eq!(cleared.retention_days, Some(None));

    let mut settings = Settings {
        retention_days: Some(30),
        ..Settings::default()
    };
    settings.change(cleared);
    assert_eq!(settings.retention_days, None);
}

#[test]
fn refuses_a_setting_it_does_not_know() {
    assert!(serde_json::from_str::<SettingsChange>(r#"{"trust": 3}"#).is_err());
    assert!(serde_json::from_str::<SettingsChange>(r#"{"trust_depth": "two"}"#).is_err());
}

#[test]
fn stores_only_the_settings_which_differ_from_the_defaults() {
    let changed = Settings {
        publish_bookmarks: false,
        trust_depth: 3,
        retention_days: Some(30),
        ..Settings::default()
    };

    let stored = changed.stored();
    assert_eq!(stored.len(), 3);
    assert_eq!(stored["publish_bookmarks"], "0");
    assert_eq!(stored["trust_depth"], "3");
    assert_eq!(stored["retention_days"], "30");
    assert_eq!(
        Settings::from_stored(&stored).expect("read settings"),
        changed
    );
}

#[test]
fn reads_the_defaults_from_an_empty_table() {
    assert_eq!(
        Settings::from_stored(&HashMap::new()).expect("read settings"),
        Settings::default()
    );

    // A default which an older version wrote is still the default
    let stored = HashMap::from([("trust_depth".to_owned(), "2".to_owned())]);
    assert_eq!(
        Settings::from_stored(&stored).expect("read settings"),
        Settings::default()
    );
}

#[test]
fn refuses_a_stored_setting_it_cannot_read() {
    let stored = HashMap::from([("trust_depth".to_owned(), "many".to_owned())]);

    let error = Settings::from_stored(&stored).expect_err("the value does not parse");
    assert!(error.to_string().contains("trust_depth"), "{error}");
}
