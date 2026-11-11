//! Opaque cursors for list pagination.
//!
//! A cursor holds the key of the last row on a page and the sort it was made
//! for, so the next page starts where the last one ended.

use serde::{Deserialize, Serialize};

use super::ApiError;

/// Unpadded base64url alphabet.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Where the page after a list answer starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    /// Sort this cursor was made for.
    pub sort: String,
    /// Sort values of the last row on the page.
    pub key: Vec<Key>,
    /// Id of the last row on the page.
    pub id: i64,
}

/// One value of a row's sort key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Key {
    /// A number for timestamps, positions and counts.
    Int(i64),
    /// Text for names and URLs.
    Text(String),
}

impl Cursor {
    /// Write this cursor into an encoded string.
    pub fn encode(&self) -> Result<String, ApiError> {
        let json = serde_json::to_vec(self)
            .map_err(|err| ApiError::internal(format!("cannot write cursor: {err}")))?;
        Ok(base64::encode(&json))
    }

    /// Read a cursor, which must have been made for `sort`.
    pub fn decode(text: &str, sort: &str) -> Result<Self, ApiError> {
        let json = base64::decode(text)
            .ok_or_else(|| ApiError::invalid(format!("cursor {text} is not base64url")))?;
        let cursor: Self = serde_json::from_slice(&json)
            .map_err(|err| ApiError::invalid(format!("cursor {text} is not a cursor: {err}")))?;
        if cursor.sort != sort {
            return Err(ApiError::invalid(format!(
                "cursor {text} was made for {}, not {sort}",
                cursor.sort
            )));
        }
        Ok(cursor)
    }
}

/// Unpadded base64url, which keeps a cursor to one line of URL-safe text.
mod base64 {
    use super::ALPHABET;

    /// Write `bytes` as unpadded base64url.
    pub(super) fn encode(bytes: &[u8]) -> String {
        let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut block = [0u8; 3];
            block[..chunk.len()].copy_from_slice(chunk);
            let bits = u32::from(block[0]) << 16 | u32::from(block[1]) << 8 | u32::from(block[2]);
            // Two characters for one byte, three for two, four for three
            for position in 0..=chunk.len() {
                text.push(ALPHABET[(bits >> (18 - position * 6)) as usize & 0x3f] as char);
            }
        }
        text
    }

    /// Read unpadded base64url or `None` when `text` is not one.
    pub(super) fn decode(text: &str) -> Option<Vec<u8>> {
        // Three bytes become four characters, so a remainder of one is impossible
        if text.len() % 4 == 1 {
            return None;
        }

        let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
        let mut bits = 0u32;
        let mut block = 0u32;
        for byte in text.bytes() {
            let value = match byte {
                b'A'..=b'Z' => byte - b'A',
                b'a'..=b'z' => byte - b'a' + 26,
                b'0'..=b'9' => byte - b'0' + 52,
                b'-' => 62,
                b'_' => 63,
                _ => return None,
            };
            block = (block << 6) | u32::from(value);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                bytes.push((block >> bits) as u8);
                block &= (1 << bits) - 1;
            }
        }
        Some(bytes)
    }
}
