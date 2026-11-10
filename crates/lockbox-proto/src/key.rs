//! Module that defines the encoding of an account keypair.
//!
//! A public key is encoded with the [`PUBLIC_PREFIX`] and a secret key with the
//! [`SECRET_PREFIX`]. Both are the raw [`KEY_LEN`] bytes in lower-case bech32m
//! (BIP-350) with a bech32m checksum.
//!
//! The pair is kept as [`SECRET_FILE`] and [`PUBLIC_FILE`] in the directory
//! named by [`crate::config`].

use std::fmt;

use bech32::{Bech32m, ByteIterExt, Fe32, Fe32IterExt, Hrp, primitives::decode::CheckedHrpstring};

/// Length of a public or secret key in bytes.
pub const KEY_LEN: usize = 32;

/// Prefix of an encoded public key.
pub const PUBLIC_PREFIX: &str = "lbox";

/// Prefix of an encoded secret key.
pub const SECRET_PREFIX: &str = "lsec";

// Both prefixes are valid ASCII, no need to check the string
const PUBLIC_HRP: Hrp = Hrp::parse_unchecked(PUBLIC_PREFIX);
const SECRET_HRP: Hrp = Hrp::parse_unchecked(SECRET_PREFIX);

/// A string that is not an encoded lockbox key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// Not a properly encoded bech32m string.
    Encoding,
    /// Encoded with an unexpected prefix.
    Prefix,
    /// Payload is not [`KEY_LEN`] bytes.
    Length,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Encoding => "not a bech32m string",
            Self::Prefix => "wrong key prefix",
            Self::Length => "wrong key length",
        };
        f.write_str(message)
    }
}

impl std::error::Error for KeyError {}

/// Encode a public key with the [`PUBLIC_PREFIX`].
pub fn encode_public(key: &[u8; KEY_LEN]) -> String {
    encode(PUBLIC_HRP, key)
}

/// Encode a secret key with the [`SECRET_PREFIX`].
pub fn encode_secret(key: &[u8; KEY_LEN]) -> String {
    encode(SECRET_HRP, key)
}

/// Decode a public key, refusing a secret key or any other prefix.
pub fn decode_public(text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    decode(PUBLIC_HRP, text)
}

/// Decode a secret key, refusing a public key or any other prefix.
pub fn decode_secret(text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    decode(SECRET_HRP, text)
}

fn encode(hrp: Hrp, key: &[u8; KEY_LEN]) -> String {
    // Skips the unnecessary `bech32::encode` checks
    key.iter()
        .copied()
        .bytes_to_fes()
        .with_checksum::<Bech32m>(&hrp)
        .chars()
        .collect()
}

fn decode(hrp: Hrp, text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    let checked = CheckedHrpstring::new::<Bech32m>(text).map_err(|_| KeyError::Encoding)?;
    if checked.hrp() != hrp {
        return Err(KeyError::Prefix);
    }
    let bytes: Vec<u8> = checked.byte_iter().collect();
    let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError::Length)?;
    // Folds padding, length and case checks into a single comparison
    if encode(hrp, &key) != text {
        return Err(KeyError::Encoding);
    }
    Ok(key)
}

/// Number of data characters a fingerprint keeps.
pub const FINGERPRINT_LEN: usize = 10;

/// Short form of a public key, for use as a quick comparison between keys.
///
/// It is the first [`FINGERPRINT_LEN`] characters of the data part of the
/// [`PUBLIC_PREFIX`] encoding, in two groups of five. A fingerprint does not
/// identify a node and is meant STRICTLY for human comparison.
pub fn fingerprint(key: &[u8; KEY_LEN]) -> String {
    let data: String = key
        .iter()
        .copied()
        .bytes_to_fes()
        .take(FINGERPRINT_LEN)
        .map(Fe32::to_char)
        .collect();
    let (head, tail) = data.split_at(data.len() / 2);
    format!("{head}-{tail}")
}

/// Name of the file that holds the encoded secret key.
pub const SECRET_FILE: &str = "secret.key";

/// Name of the file that holds the encoded public key.
pub const PUBLIC_FILE: &str = "public.key";
