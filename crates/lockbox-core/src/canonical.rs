//! Canonical forms of the addresses the reader saves.

use std::fmt;

use url::Url;

/// Query parameters which are commonly used for tracking request origins.
const TRACKING: [&str; 7] = [
    "fbclid", "gclid", "igshid", "mc_cid", "mc_eid", "ref_src", "si",
];

/// Rewrite a link into the form which decides whether it is already saved.
pub fn canonical(text: &str) -> Result<String, Invalid> {
    let mut url = Url::parse(text.trim()).map_err(|err| Invalid(format!("{text}: {err}")))?;
    match url.scheme() {
        "http" | "https" => {}
        "gopher" | "gemini" => {
            if url.host_str().is_none_or(str::is_empty)
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err(Invalid(format!("{text} has an invalid authority")));
            }
            return Ok(url.to_string());
        }
        _ => return Err(Invalid(format!("{text} has an unsupported scheme"))),
    }

    url.set_fragment(None);
    let host = url
        .host_str()
        .and_then(|host| host.strip_prefix("www."))
        .map(str::to_owned);
    if let Some(host) = host {
        url.set_host(Some(&host))
            .map_err(|err| Invalid(format!("{text}: {err}")))?;
    }

    if url.query().is_some() {
        let mut pairs: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(key, _)| !tracking(key))
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        pairs.sort();
        if pairs.is_empty() {
            url.set_query(None);
        } else {
            url.query_pairs_mut().clear().extend_pairs(pairs);
        }
    }

    let path = url.path().to_owned();
    if path.len() > 1 && path.ends_with('/') {
        url.set_path(path.trim_end_matches('/'));
    }

    Ok(url.to_string())
}

/// Whether a query parameter is a tracking marker.
fn tracking(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_") || TRACKING.contains(&key.as_str())
}

/// A link which cannot be saved under a canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(String);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Invalid {}
