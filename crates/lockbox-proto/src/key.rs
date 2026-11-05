//! Module that defines the encoding of an account keypair.
//!
//! A public key is encoded with the [`PUBLIC_PREFIX`] and a secret key with the
//! [`SECRET_PREFIX`]. Both are the raw [`KEY_LEN`] bytes in lower-case bech32m
//! (BIP-350) with a bech32m checksum.
//!
//! The pair is kept as [`SECRET_FILE`] and [`PUBLIC_FILE`] in
//! [`crate::config::dir`], written once by [`write_secret`] and
//! [`write_public`], and read back with [`read_secret`].

use std::{
    fmt, fs,
    io::{self, Write},
    path::Path,
};

use bech32::{Bech32m, Hrp, primitives::decode::CheckedHrpstring};

/// Length of a public or secret key in bytes.
pub const KEY_LEN: usize = 32;

/// Prefix of an encoded public key.
pub const PUBLIC_PREFIX: &str = "lbox0";

/// Prefix of an encoded secret key.
pub const SECRET_PREFIX: &str = "lsec0";

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
    encode(PUBLIC_PREFIX, key)
}

/// Encode a secret key with the [`SECRET_PREFIX`].
pub fn encode_secret(key: &[u8; KEY_LEN]) -> String {
    encode(SECRET_PREFIX, key)
}

/// Decode a public key, refusing a secret key or any other prefix.
pub fn decode_public(text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    decode(PUBLIC_PREFIX, text)
}

/// Decode a secret key, refusing a public key or any other prefix.
pub fn decode_secret(text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    decode(SECRET_PREFIX, text)
}

fn encode(prefix: &str, key: &[u8; KEY_LEN]) -> String {
    bech32::encode::<Bech32m>(hrp(prefix), key).expect("a key fits in a bech32m string")
}

fn decode(prefix: &str, text: &str) -> Result<[u8; KEY_LEN], KeyError> {
    let checked = CheckedHrpstring::new::<Bech32m>(text).map_err(|_| KeyError::Encoding)?;
    if checked.hrp() != hrp(prefix) {
        return Err(KeyError::Prefix);
    }
    let bytes: Vec<u8> = checked.byte_iter().collect();
    let key: [u8; KEY_LEN] = bytes.try_into().map_err(|_| KeyError::Length)?;
    // Folds padding, length and case checks into a single comparison
    if encode(prefix, &key) != text {
        return Err(KeyError::Encoding);
    }
    Ok(key)
}

fn hrp(prefix: &str) -> Hrp {
    Hrp::parse(prefix).expect("both key prefixes are valid human readable parts")
}

/// Name of the file that holds the encoded secret key.
pub const SECRET_FILE: &str = "secret.key";

/// Name of the file that holds the encoded public key.
pub const PUBLIC_FILE: &str = "public.key";

/// Why the secret key file could not be read.
#[derive(Debug)]
pub enum ReadError {
    /// There is no secret key file.
    Missing,
    /// The file does not hold an encoded secret key.
    Key(KeyError),
    /// The file could not be read.
    Io(io::Error),
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("no secret key file"),
            Self::Key(err) => err.fmt(f),
            Self::Io(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for ReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Missing => None,
            Self::Key(err) => Some(err),
            Self::Io(err) => Some(err),
        }
    }
}

/// Read the secret key from the file in `dir`.
pub fn read_secret(dir: &Path) -> Result<[u8; KEY_LEN], ReadError> {
    let text = match fs::read_to_string(dir.join(SECRET_FILE)) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(ReadError::Missing),
        Err(err) => return Err(ReadError::Io(err)),
    };
    decode_secret(text.trim()).map_err(ReadError::Key)
}

/// Write the secret key into a file in `dir`.
///
/// The file is owner-only and an existing one is never replaced.
pub fn write_secret(dir: &Path, key: &[u8; KEY_LEN]) -> io::Result<()> {
    create(&dir.join(SECRET_FILE), &encode_secret(key), 0o600)
}

/// Write the public key into a file in `dir`. An existing one is never
/// replaced.
pub fn write_public(dir: &Path, key: &[u8; KEY_LEN]) -> io::Result<()> {
    create(&dir.join(PUBLIC_FILE), &encode_public(key), 0o644)
}

/// Create a file in `path` with `text` in it, and `mode` as its permission bits
/// where the platform has them.
#[cfg_attr(not(unix), allow(unused_variables))]
fn create(path: &Path, text: &str, mode: u32) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{text}")
}
