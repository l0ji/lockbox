//! The node and error pages.

use askama::Template;
use lockbox_proto::api::node::Status;

/// Page which reports on the daemon itself.
pub const NODE_PAGE: &str = "/node";

/// Template context for the status page.
#[derive(Template)]
#[template(path = "node.html")]
pub struct Page {
    /// Section of the site which the navbar marks.
    section: &'static str,
    /// Either `live` or `down`, which styles the status dot.
    state: &'static str,
    /// Text next to the status dot.
    state_text: &'static str,
    public_key: String,
    node_id: String,
    protocol: String,
    software: String,
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
            section: "node",
            state: "live",
            state_text: "live",
            public_key: status.public_key,
            node_id: status.node_id,
            protocol: status.protocol_version.to_string(),
            software: status.software_version,
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
            section: "node",
            state: "down",
            state_text: "daemon offline",
            public_key: "-".to_owned(),
            node_id: "-".to_owned(),
            protocol: "-".to_owned(),
            software: "-".to_owned(),
            uptime: "-".to_owned(),
            has_relays: false,
            relays: Vec::new(),
            has_addrs: false,
            addrs: Vec::new(),
            error: error.into(),
        }
    }
}

/// Template context for a page which failed.
#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorPage {
    /// Section of the site which the navbar marks.
    pub section: &'static str,
    /// Status which the browser sees.
    pub status: u16,
    /// Reason to show on the page.
    pub message: String,
    /// Whether the daemon is not running at all.
    pub offline: bool,
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
