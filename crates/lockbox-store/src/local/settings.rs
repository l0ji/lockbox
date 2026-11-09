//! The user's settings, preset with fallback defaults.

use std::{collections::HashMap, str::FromStr};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{Db, Error};

/// Everything the user can tune, read when started by the daemon.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// Saving a bookmark publishes it to the federated network.
    pub publish_bookmarks: bool,
    /// How deep a follower/follwing graph should go before it stops downloading
    /// the complete repositories of users.
    pub trust_depth: i64,
    /// Purge read, unstarred entries older than this many days.
    pub retention_days: Option<i64>,
    /// How long `unknown` content survives.
    pub unknown_retention_days: i64,
    /// Stored bytes per `unknown` author.
    pub unknown_cap_bytes: i64,
    /// Stored bytes across all `unknown` authors.
    pub unknown_total_bytes: i64,
    /// Share of a link's score that comes from quality.
    pub rank_quality_weight: f64,
    /// Share that comes from being on-topic.
    pub rank_topic_weight: f64,
    /// Strength of agreement, on top of the base.
    pub rank_agree_weight: f64,
    /// Age at which a link's rank halves.
    pub rank_half_life_hours: i64,
    /// How sharply age cuts the score.
    pub rank_decay_exponent: f64,
    /// How long a deletion is remembered.
    pub tombstone_days: i64,
    /// How often a public feed may refresh its site record.
    pub site_refresh_seconds: i64,
    /// How long another author's entry records are kept.
    pub entry_retention_days: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            publish_bookmarks: true,
            trust_depth: 2,
            retention_days: None,
            unknown_retention_days: 180,
            unknown_cap_bytes: 8_388_608,
            unknown_total_bytes: 536_870_912,
            rank_quality_weight: 0.6,
            rank_topic_weight: 0.4,
            rank_agree_weight: 0.05,
            rank_half_life_hours: 48,
            rank_decay_exponent: 1.8,
            tombstone_days: 365,
            site_refresh_seconds: 3600,
            entry_retention_days: 90,
        }
    }
}

/// Read the settings, with the defaults filled in.
pub async fn read(db: &Db) -> Result<Settings, Error> {
    db.call(|connection| load(connection)).await
}

/// Store the settings, keeping only the values which differ from the what is
/// already set.
pub async fn write(db: &Db, settings: &Settings) -> Result<(), Error> {
    let settings = settings.clone();
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        let defaults = Settings::default();

        for ((key, value), (_, default)) in settings.pairs().into_iter().zip(defaults.pairs()) {
            keep(&transaction, key, value, default)?;
        }

        transaction.commit()?;
        Ok(())
    })
    .await
}

/// Whether saving a bookmark publishes it to the federated network.
pub(super) fn publish_bookmarks(connection: &Connection) -> Result<bool, Error> {
    let stored: Option<String> = connection
        .query_row(
            "SELECT value FROM setting WHERE key = 'publish_bookmarks'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    match stored {
        Some(text) => flag("publish_bookmarks", &text),
        None => Ok(Settings::default().publish_bookmarks),
    }
}

/// Read every stored setting.
fn load(connection: &Connection) -> Result<Settings, Error> {
    let mut stored: HashMap<String, String> = HashMap::new();
    let mut statement = connection.prepare("SELECT key, value FROM setting")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        stored.insert(key, value);
    }

    let defaults = Settings::default();
    Ok(Settings {
        publish_bookmarks: match stored.get("publish_bookmarks") {
            Some(text) => flag("publish_bookmarks", text)?,
            None => defaults.publish_bookmarks,
        },
        trust_depth: parse(&stored, "trust_depth", defaults.trust_depth)?,
        retention_days: match stored.get("retention_days") {
            Some(text) => Some(value("retention_days", text)?),
            None => None,
        },
        unknown_retention_days: parse(
            &stored,
            "unknown_retention_days",
            defaults.unknown_retention_days,
        )?,
        unknown_cap_bytes: parse(&stored, "unknown_cap_bytes", defaults.unknown_cap_bytes)?,
        unknown_total_bytes: parse(&stored, "unknown_total_bytes", defaults.unknown_total_bytes)?,
        rank_quality_weight: parse(&stored, "rank_quality_weight", defaults.rank_quality_weight)?,
        rank_topic_weight: parse(&stored, "rank_topic_weight", defaults.rank_topic_weight)?,
        rank_agree_weight: parse(&stored, "rank_agree_weight", defaults.rank_agree_weight)?,
        rank_half_life_hours: parse(
            &stored,
            "rank_half_life_hours",
            defaults.rank_half_life_hours,
        )?,
        rank_decay_exponent: parse(&stored, "rank_decay_exponent", defaults.rank_decay_exponent)?,
        tombstone_days: parse(&stored, "tombstone_days", defaults.tombstone_days)?,
        site_refresh_seconds: parse(
            &stored,
            "site_refresh_seconds",
            defaults.site_refresh_seconds,
        )?,
        entry_retention_days: parse(
            &stored,
            "entry_retention_days",
            defaults.entry_retention_days,
        )?,
    })
}

/// Store one setting, or drop it when it is back at its default.
fn keep(connection: &Connection, key: &str, value: String, default: String) -> Result<(), Error> {
    if value == default {
        connection.execute("DELETE FROM setting WHERE key = ?1", [key])?;
    } else {
        connection.execute(
            "INSERT INTO setting (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
    }
    Ok(())
}

impl Settings {
    /// Every setting as its key and its stored text.
    fn pairs(&self) -> [(&'static str, String); 14] {
        [
            (
                "publish_bookmarks",
                flag_text(self.publish_bookmarks).to_owned(),
            ),
            ("trust_depth", self.trust_depth.to_string()),
            (
                "retention_days",
                self.retention_days
                    .map(|days| days.to_string())
                    .unwrap_or_default(),
            ),
            (
                "unknown_retention_days",
                self.unknown_retention_days.to_string(),
            ),
            ("unknown_cap_bytes", self.unknown_cap_bytes.to_string()),
            ("unknown_total_bytes", self.unknown_total_bytes.to_string()),
            ("rank_quality_weight", self.rank_quality_weight.to_string()),
            ("rank_topic_weight", self.rank_topic_weight.to_string()),
            ("rank_agree_weight", self.rank_agree_weight.to_string()),
            (
                "rank_half_life_hours",
                self.rank_half_life_hours.to_string(),
            ),
            ("rank_decay_exponent", self.rank_decay_exponent.to_string()),
            ("tombstone_days", self.tombstone_days.to_string()),
            (
                "site_refresh_seconds",
                self.site_refresh_seconds.to_string(),
            ),
            (
                "entry_retention_days",
                self.entry_retention_days.to_string(),
            ),
        ]
    }
}

/// Read one setting, with a default when it was never stored.
fn parse<T: FromStr>(stored: &HashMap<String, String>, key: &str, default: T) -> Result<T, Error> {
    match stored.get(key) {
        Some(text) => value(key, text),
        None => Ok(default),
    }
}

/// Parse one stored value, naming the setting when it does not fit.
fn value<T: FromStr>(key: &str, text: &str) -> Result<T, Error> {
    text.parse()
        .map_err(|_| Error::Invalid(format!("setting {key} does not hold {text}")))
}

/// Parse a stored switch.
fn flag(key: &str, text: &str) -> Result<bool, Error> {
    match text {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(Error::Invalid(format!(
            "setting {key} does not hold {other}"
        ))),
    }
}

/// How a switch is stored.
fn flag_text(value: bool) -> &'static str {
    match value {
        true => "1",
        false => "0",
    }
}
