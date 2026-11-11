//! Parsing of feeds and the pages which advertise a link to their feed.

use std::fmt;

use feed_rs::{
    model::{Entry as Document, FeedType, Link},
    parser::ParseFeedError,
};
use html_escape::decode_html_entities;
use lockbox_proto::api::reader::Kind;
use url::Url;

/// Document types which a page advertises its feed as.
const FEED_TYPES: [&str; 5] = [
    "application/rss+xml",
    "application/atom+xml",
    "application/feed+json",
    "application/json",
    "application/xml",
];

/// What one feed document holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// Format the document was written in.
    pub kind: Kind,
    /// Title of the feed itself.
    pub title: Option<String>,
    /// Site the feed belongs to.
    pub site_url: Option<String>,
    /// Items the document holds.
    pub entries: Vec<Item>,
}

/// One item of a feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Identifier which the feed gives the item.
    pub source: String,
    /// Page the item links to.
    pub url: Option<String>,
    /// Title of the item, which falls back to its address.
    pub title: String,
    /// Who wrote it.
    pub author: Option<String>,
    /// Short summary the feed gave.
    pub summary: Option<String>,
    /// Body as the document gave it.
    pub body: Option<String>,
    /// When the feed says it was published.
    pub published_at: Option<i64>,
}

/// Read a feed document, whose format the parser works out itself.
///
/// `base` resolves the relative addresses which a document may hold.
pub fn parse(body: &[u8], base: Option<&Url>) -> Result<Parsed, Error> {
    let mut builder = feed_rs::parser::Builder::new();
    if let Some(base) = base {
        builder = builder.base_uri(Some(base.as_str()));
    }
    let feed = builder.build().parse(body).map_err(Error)?;

    let kind = kind(feed.feed_type);
    let title = text(feed.title);
    let site_url = link(&feed.links);
    let author = feed.authors.first().map(|person| decode(&person.name));
    let entries = feed
        .entries
        .into_iter()
        .map(|entry| item(entry, author.clone()))
        .collect();

    Ok(Parsed {
        kind,
        title,
        site_url,
        entries,
    })
}

/// Address of the feed which a page advertises, if it advertises one.
///
/// `base` resolves the address which the page holds, which is often relative.
pub fn discover(page: &str, base: &Url) -> Option<Url> {
    let mut from = 0;
    while let Some(offset) = page[from..].find('<') {
        let start = from + offset + 1;
        let length = page[start..].find('>')?;
        let tag = &page[start..start + length];
        from = start + length;

        // Tag names are written in any case, so only the attributes keep theirs
        let name_end = tag
            .find(|byte: char| byte.is_whitespace() || byte == '/')
            .unwrap_or(tag.len());
        if !tag[..name_end].eq_ignore_ascii_case("link") {
            continue;
        }
        let attributes = attributes(&tag[name_end..]);
        let rel = attribute(&attributes, "rel");
        let media_type = attribute(&attributes, "type");
        let href = attribute(&attributes, "href");

        let advertised = rel.is_some_and(|rel| {
            rel.split_whitespace()
                .any(|part| part.eq_ignore_ascii_case("alternate"))
        });
        let is_feed = media_type.is_some_and(|media_type| {
            let media_type = media_type.split(';').next().unwrap_or_default().trim();
            FEED_TYPES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(media_type))
        });
        if !advertised || !is_feed {
            continue;
        }
        if let Some(url) = href.and_then(|href| base.join(href).ok()) {
            return Some(url);
        }
    }
    None
}

/// Read one item of a document, which falls back to the feed's author.
fn item(entry: Document, author: Option<String>) -> Item {
    let url = link(&entry.links);
    let title = text(entry.title)
        .or_else(|| url.clone())
        .unwrap_or_else(|| entry.id.clone());

    Item {
        source: entry.id,
        url,
        title,
        author: entry
            .authors
            .first()
            .map(|person| decode(&person.name))
            .or(author),
        summary: text(entry.summary),
        body: entry
            .content
            .and_then(|content| content.body)
            .map(|body| decode(&body)),
        published_at: entry
            .published
            .or(entry.updated)
            .map(|date| date.timestamp()),
    }
}

/// The feed the model identified the document as, as the API names it.
fn kind(feed_type: FeedType) -> Kind {
    match feed_type {
        FeedType::Atom => Kind::Atom,
        FeedType::JSON => Kind::Json,
        FeedType::RSS0 | FeedType::RSS1 | FeedType::RSS2 => Kind::Rss,
    }
}

/// Text of a document field, where an empty one counts as missing.
fn text(text: Option<feed_rs::model::Text>) -> Option<String> {
    text.map(|text| decode(&text.content))
        .filter(|content| !content.trim().is_empty())
}

/// A piece of text with the character references which it writes decoded.
///
/// A document may escape its text once, or twice over when it was built by
/// something which escaped it already, so one pass turns both into the
/// characters a reader should see.
fn decode(text: &str) -> String {
    match text.contains('&') {
        true => decode_html_entities(text).into_owned(),
        false => text.to_owned(),
    }
}

/// Address of the page which the links of a document point at.
fn link(links: &[Link]) -> Option<String> {
    links
        .iter()
        .find(|link| matches!(link.rel.as_deref(), None | Some("alternate")))
        .map(|link| link.href.trim().to_owned())
        .filter(|href| !href.is_empty())
}

/// Attributes of one tag, as they were written.
fn attributes(tag: &str) -> Vec<(&str, &str)> {
    let mut attributes = Vec::new();
    let mut rest = tag.trim_end_matches('/');
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }

        let name_end = rest
            .find(|byte: char| byte.is_whitespace() || byte == '=')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        rest = rest[name_end..].trim_start();

        let Some(after) = rest.strip_prefix('=') else {
            attributes.push((name, ""));
            continue;
        };
        let (value, remainder) = value(after.trim_start());
        attributes.push((name, value));
        rest = remainder;
    }
    attributes
}

/// Read one attribute value, which may be quoted or bare.
fn value(text: &str) -> (&str, &str) {
    match text.chars().next() {
        Some(quote @ ('"' | '\'')) => match text[1..].find(quote) {
            Some(end) => (&text[1..1 + end], &text[2 + end..]),
            None => (&text[1..], ""),
        },
        _ => {
            let end = text.find(char::is_whitespace).unwrap_or(text.len());
            (&text[..end], &text[end..])
        }
    }
}

/// Value of one attribute of a tag.
fn attribute<'a>(attributes: &[(&'a str, &'a str)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

/// Why a document could not be read as a feed.
#[derive(Debug)]
pub struct Error(ParseFeedError);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}
