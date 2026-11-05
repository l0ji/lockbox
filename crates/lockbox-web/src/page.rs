//! The single page the web client serves.

use askama::Template;
use lockbox_proto::api::Status;

/// Template context for the status page.
#[derive(Template)]
#[template(path = "index.html")]
pub struct Page {
    /// Either `live` or `down`, which styles the status dot.
    state: &'static str,
    /// Text next to the status dot.
    state_text: &'static str,
    node_id: String,
    protocol: String,
    daemon: String,
    uptime: String,
    has_relays: bool,
    relays: Vec<String>,
    has_addrs: bool,
    addrs: Vec<String>,
    /// Rendered in the error line and hidden by CSS when empty.
    error: String,
}

impl Page {
    /// Page for a daemon that answered.
    pub fn live(status: Status) -> Self {
        Self {
            state: "live",
            state_text: "live",
            node_id: status.node_id,
            protocol: status.protocol_version.to_string(),
            daemon: status.daemon_version,
            uptime: format_uptime(status.uptime_secs),
            has_relays: !status.relay_urls.is_empty(),
            relays: status.relay_urls,
            has_addrs: !status.direct_addrs.is_empty(),
            addrs: status.direct_addrs,
            error: String::new(),
        }
    }

    /// Page for a daemon that did not answer.
    pub fn down(error: impl Into<String>) -> Self {
        Self {
            state: "down",
            state_text: "daemon unreachable",
            node_id: "-".to_owned(),
            protocol: "-".to_owned(),
            daemon: "-".to_owned(),
            uptime: "-".to_owned(),
            has_relays: false,
            relays: Vec::new(),
            has_addrs: false,
            addrs: Vec::new(),
            error: error.into(),
        }
    }
}

/// Uptime in seconds, in the largest two units that fit.
fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    let minutes = secs / 60;
    if minutes < 60 {
        return format!("{minutes}m {}s", secs % 60);
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h {}m", minutes % 60);
    }
    format!("{}d {}h", hours / 24, hours % 24)
}
