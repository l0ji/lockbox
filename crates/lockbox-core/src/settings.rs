//! The node's (and user's) current settings. All settings are automatically
//! configured to some kind of default.

use std::{collections::HashMap, fmt, str::FromStr};

use lockbox_types::nullable;
use serde::{Deserialize, Serialize};

/// All of the preferences that the user can tune.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Whether saving a bookmark automatically publishes it to the federated
    /// network.
    pub publish_bookmarks: bool,
    /// How deep a follower/following graph should go before it stops
    /// auto-downloading the complete repositories of users.
    pub trust_depth: i64,
    /// Purge read, unstarred entries older than this many days.
    pub retention_days: Option<i64>,
    /// How long content from unknown publishers survives.
    pub unknown_retention_days: i64,
    /// Storage cap per unknown author.
    pub unknown_cap_bytes: i64,
    /// Storage cap across all unknown authors.
    pub unknown_total_bytes: i64,
    /// Share of a link's score that comes from quality.
    pub rank_quality_weight: f64,
    /// Share of a link's score that comes from being on-topic.
    pub rank_topic_weight: f64,
    /// Share of a link's score that comes from general agreement with the
    /// contents.
    pub rank_agree_weight: f64,
    /// Time after which a link's rank halves.
    pub rank_half_life_hours: i64,
    /// How sharply a link's age cuts the ranking score.
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

impl Settings {
    /// Apply the settings that `change` names and leave every other one alone.
    pub fn change(&mut self, change: SettingsChange) {
        if let Some(value) = change.publish_bookmarks {
            self.publish_bookmarks = value;
        }
        if let Some(value) = change.trust_depth {
            self.trust_depth = value;
        }
        if let Some(value) = change.retention_days {
            self.retention_days = value;
        }
        if let Some(value) = change.unknown_retention_days {
            self.unknown_retention_days = value;
        }
        if let Some(value) = change.unknown_cap_bytes {
            self.unknown_cap_bytes = value;
        }
        if let Some(value) = change.unknown_total_bytes {
            self.unknown_total_bytes = value;
        }
        if let Some(value) = change.rank_quality_weight {
            self.rank_quality_weight = value;
        }
        if let Some(value) = change.rank_topic_weight {
            self.rank_topic_weight = value;
        }
        if let Some(value) = change.rank_agree_weight {
            self.rank_agree_weight = value;
        }
        if let Some(value) = change.rank_half_life_hours {
            self.rank_half_life_hours = value;
        }
        if let Some(value) = change.rank_decay_exponent {
            self.rank_decay_exponent = value;
        }
        if let Some(value) = change.tombstone_days {
            self.tombstone_days = value;
        }
        if let Some(value) = change.site_refresh_seconds {
            self.site_refresh_seconds = value;
        }
        if let Some(value) = change.entry_retention_days {
            self.entry_retention_days = value;
        }
    }

    /// Read the settings from the text that is stored.
    ///
    /// A setting which was never stored keeps its default, so changing a
    /// default here reaches every user who did not set it themselves.
    pub fn from_stored(stored: &HashMap<String, String>) -> Result<Self, Invalid> {
        let defaults = Self::default();
        Ok(Self {
            publish_bookmarks: match stored.get("publish_bookmarks") {
                Some(text) => flag("publish_bookmarks", text)?,
                None => defaults.publish_bookmarks,
            },
            trust_depth: parse(stored, "trust_depth", defaults.trust_depth)?,
            retention_days: match stored.get("retention_days") {
                Some(text) => Some(value("retention_days", text)?),
                None => None,
            },
            unknown_retention_days: parse(
                stored,
                "unknown_retention_days",
                defaults.unknown_retention_days,
            )?,
            unknown_cap_bytes: parse(stored, "unknown_cap_bytes", defaults.unknown_cap_bytes)?,
            unknown_total_bytes: parse(
                stored,
                "unknown_total_bytes",
                defaults.unknown_total_bytes,
            )?,
            rank_quality_weight: parse(
                stored,
                "rank_quality_weight",
                defaults.rank_quality_weight,
            )?,
            rank_topic_weight: parse(stored, "rank_topic_weight", defaults.rank_topic_weight)?,
            rank_agree_weight: parse(stored, "rank_agree_weight", defaults.rank_agree_weight)?,
            rank_half_life_hours: parse(
                stored,
                "rank_half_life_hours",
                defaults.rank_half_life_hours,
            )?,
            rank_decay_exponent: parse(
                stored,
                "rank_decay_exponent",
                defaults.rank_decay_exponent,
            )?,
            tombstone_days: parse(stored, "tombstone_days", defaults.tombstone_days)?,
            site_refresh_seconds: parse(
                stored,
                "site_refresh_seconds",
                defaults.site_refresh_seconds,
            )?,
            entry_retention_days: parse(
                stored,
                "entry_retention_days",
                defaults.entry_retention_days,
            )?,
        })
    }

    /// The settings which differ from their defaults as they are stored.
    pub fn stored(&self) -> HashMap<String, String> {
        let defaults = Settings::default();
        pairs(self)
            .into_iter()
            .zip(pairs(&defaults))
            .filter(|((_, value), (_, default))| value != default)
            .map(|((key, value), _)| (key.to_owned(), value))
            .collect()
    }
}

/// Settings to change, where a missing field keeps its current value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsChange {
    pub publish_bookmarks: Option<bool>,
    pub trust_depth: Option<i64>,
    #[serde(default, deserialize_with = "nullable")]
    pub retention_days: Option<Option<i64>>,
    pub unknown_retention_days: Option<i64>,
    pub unknown_cap_bytes: Option<i64>,
    pub unknown_total_bytes: Option<i64>,
    pub rank_quality_weight: Option<f64>,
    pub rank_topic_weight: Option<f64>,
    pub rank_agree_weight: Option<f64>,
    pub rank_half_life_hours: Option<i64>,
    pub rank_decay_exponent: Option<f64>,
    pub tombstone_days: Option<i64>,
    pub site_refresh_seconds: Option<i64>,
    pub entry_retention_days: Option<i64>,
}

/// Error for a stored setting which does not hold what it should.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}

/// Every setting as its key and stored value.
fn pairs(settings: &Settings) -> [(&'static str, String); 14] {
    [
        (
            "publish_bookmarks",
            flag_text(settings.publish_bookmarks).to_owned(),
        ),
        ("trust_depth", settings.trust_depth.to_string()),
        (
            "retention_days",
            settings
                .retention_days
                .map(|days| days.to_string())
                .unwrap_or_default(),
        ),
        (
            "unknown_retention_days",
            settings.unknown_retention_days.to_string(),
        ),
        ("unknown_cap_bytes", settings.unknown_cap_bytes.to_string()),
        (
            "unknown_total_bytes",
            settings.unknown_total_bytes.to_string(),
        ),
        (
            "rank_quality_weight",
            settings.rank_quality_weight.to_string(),
        ),
        ("rank_topic_weight", settings.rank_topic_weight.to_string()),
        ("rank_agree_weight", settings.rank_agree_weight.to_string()),
        (
            "rank_half_life_hours",
            settings.rank_half_life_hours.to_string(),
        ),
        (
            "rank_decay_exponent",
            settings.rank_decay_exponent.to_string(),
        ),
        ("tombstone_days", settings.tombstone_days.to_string()),
        (
            "site_refresh_seconds",
            settings.site_refresh_seconds.to_string(),
        ),
        (
            "entry_retention_days",
            settings.entry_retention_days.to_string(),
        ),
    ]
}

/// Read one setting, returning a default if it was never stored.
fn parse<T: FromStr>(
    stored: &HashMap<String, String>,
    key: &str,
    default: T,
) -> Result<T, Invalid> {
    match stored.get(key) {
        Some(text) => value(key, text),
        None => Ok(default),
    }
}

/// Read one stored value, returning the setting's name if invalid.
fn value<T: FromStr>(key: &str, text: &str) -> Result<T, Invalid> {
    text.parse()
        .map_err(|_| Invalid(format!("setting {key} does not hold {text}")))
}

/// Read a stored switch.
fn flag(key: &str, text: &str) -> Result<bool, Invalid> {
    match text {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(Invalid(format!("setting {key} does not hold {other}"))),
    }
}

/// How a switch is stored.
fn flag_text(value: bool) -> &'static str {
    match value {
        true => "1",
        false => "0",
    }
}
