//! This module contains the functions to read and write keypairs to files.

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use lockbox_proto::key::{self, KeyError};
pub use lockbox_proto::key::{KEY_LEN, PUBLIC_FILE, SECRET_FILE};

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

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
    key::decode_secret(text.trim()).map_err(ReadError::Key)
}

/// Write the secret key into a file in `dir`.
///
/// The file is owner-only and refuses to replace an existing keypair.
pub fn write_secret(dir: &Path, secret: &[u8; KEY_LEN]) -> io::Result<()> {
    create(&dir.join(SECRET_FILE), &key::encode_secret(secret), 0o600)
}

/// Write the public key into a file in `dir`.
/// This function refuses to replace an existing keypair.
pub fn write_public(dir: &Path, public: &[u8; KEY_LEN]) -> io::Result<()> {
    create(&dir.join(PUBLIC_FILE), &key::encode_public(public), 0o644)
}

/// Create a file in `path` with `text` in it, and `mode` as its permission bits
/// when the platform has them.
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
