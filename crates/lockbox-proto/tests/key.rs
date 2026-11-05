//! Tests for [`lockbox_proto::key`].

mod common;

use std::{fs, io};

use common::Temp;
use lockbox_proto::key::{
    self, KEY_LEN, KeyError, PUBLIC_FILE, PUBLIC_PREFIX, ReadError, SECRET_FILE, SECRET_PREFIX,
};

/// Bytes 0-31, which encode to the vectors below.
const BYTES: [u8; KEY_LEN] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];

const PUBLIC: &str = "lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sw7hefe";
const SECRET: &str = "lsec01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sjhjmgt";

#[test]
fn encodes_a_public_key() {
    assert_eq!(key::encode_public(&BYTES), PUBLIC);
    assert_eq!(PUBLIC.len(), 64);
}

#[test]
fn encodes_a_secret_key() {
    assert_eq!(key::encode_secret(&BYTES), SECRET);
}

#[test]
fn decodes_both_vectors() {
    assert_eq!(key::decode_public(PUBLIC), Ok(BYTES));
    assert_eq!(key::decode_secret(SECRET), Ok(BYTES));
}

#[test]
fn round_trips_arbitrary_keys() {
    for fill in [0x00, 0x7f, 0xff] {
        let bytes = [fill; KEY_LEN];
        assert_eq!(key::decode_public(&key::encode_public(&bytes)), Ok(bytes));
        assert_eq!(key::decode_secret(&key::encode_secret(&bytes)), Ok(bytes));
    }
    assert_eq!(
        key::encode_public(&[0; KEY_LEN]),
        "lbox01qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq82umfq"
    );
}

#[test]
fn prefixes_are_kept_apart() {
    assert_eq!(key::decode_public(SECRET), Err(KeyError::Prefix));
    assert_eq!(key::decode_secret(PUBLIC), Err(KeyError::Prefix));
    assert!(key::encode_public(&BYTES).starts_with(PUBLIC_PREFIX));
    assert!(key::encode_secret(&BYTES).starts_with(SECRET_PREFIX));
}

#[test]
fn refuses_a_broken_checksum() {
    let broken = "lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sw7hefq";
    assert_eq!(key::decode_public(broken), Err(KeyError::Encoding));
}

#[test]
fn refuses_a_plain_bech32_checksum() {
    let bech32 = "lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0smz84vm";
    assert_eq!(key::decode_public(bech32), Err(KeyError::Encoding));
}

#[test]
fn refuses_upper_case() {
    let upper = "LBOX01QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SW7HEFE";
    assert_eq!(key::decode_public(upper), Err(KeyError::Encoding));
}

#[test]
fn refuses_a_payload_that_is_not_a_key() {
    let short = "lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysne7flpx";
    assert_eq!(key::decode_public(short), Err(KeyError::Length));
}

#[test]
fn refuses_plain_text() {
    assert_eq!(key::decode_public("not a key"), Err(KeyError::Encoding));
    assert_eq!(key::decode_secret(""), Err(KeyError::Encoding));
}

#[test]
fn writes_and_reads_the_secret() {
    let temp = Temp::new("secret-roundtrip");
    key::write_secret(temp.path(), &BYTES).expect("write secret");
    assert_eq!(key::read_secret(temp.path()).expect("read secret"), BYTES);
    let filed = fs::read_to_string(temp.path().join(SECRET_FILE)).expect("read secret file");
    assert_eq!(filed, format!("{SECRET}\n"));
}

#[test]
fn writes_the_public_key_beside_the_secret() {
    let temp = Temp::new("public-roundtrip");
    key::write_public(temp.path(), &BYTES).expect("write public");
    let filed = fs::read_to_string(temp.path().join(PUBLIC_FILE)).expect("read public file");
    assert_eq!(filed, format!("{PUBLIC}\n"));
    assert!(filed.trim().starts_with(PUBLIC_PREFIX));
}

#[cfg(unix)]
#[test]
fn the_secret_file_is_owner_only() {
    let temp = Temp::new("secret-mode");
    key::write_secret(temp.path(), &BYTES).expect("write secret");
    assert_eq!(mode(&temp.path().join(SECRET_FILE)), 0o600);
}

#[cfg(unix)]
#[test]
fn the_public_file_is_readable() {
    let temp = Temp::new("public-mode");
    key::write_public(temp.path(), &BYTES).expect("write public");
    assert_eq!(mode(&temp.path().join(PUBLIC_FILE)), 0o644);
}

#[test]
fn an_existing_file_is_never_replaced() {
    let temp = Temp::new("no-replace");
    key::write_secret(temp.path(), &BYTES).expect("write secret");
    let err = key::write_secret(temp.path(), &[0x11; KEY_LEN]).expect_err("second secret");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
    key::write_public(temp.path(), &BYTES).expect("write public");
    let err = key::write_public(temp.path(), &[0x11; KEY_LEN]).expect_err("second public");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_missing_secret_is_reported_as_missing() {
    let temp = Temp::new("no-secret");
    assert!(matches!(
        key::read_secret(temp.path()),
        Err(ReadError::Missing)
    ));
}

#[test]
fn a_corrupt_secret_is_refused() {
    let temp = Temp::new("corrupt-secret");
    fs::write(temp.path().join(SECRET_FILE), "not a key\n").expect("write garbage");
    assert!(matches!(
        key::read_secret(temp.path()),
        Err(ReadError::Key(KeyError::Encoding))
    ));
}

#[test]
fn a_public_key_in_the_secret_file_is_refused() {
    let temp = Temp::new("public-as-secret");
    fs::write(temp.path().join(SECRET_FILE), PUBLIC).expect("write public key");
    assert!(matches!(
        key::read_secret(temp.path()),
        Err(ReadError::Key(KeyError::Prefix))
    ));
}

#[test]
fn a_second_line_is_refused() {
    let temp = Temp::new("two-lines");
    fs::write(
        temp.path().join(SECRET_FILE),
        format!("{SECRET}\n{SECRET}\n"),
    )
    .expect("write two");
    assert!(matches!(
        key::read_secret(temp.path()),
        Err(ReadError::Key(KeyError::Encoding))
    ));
}

#[test]
fn surrounding_whitespace_is_ignored() {
    let temp = Temp::new("padded-secret");
    fs::write(temp.path().join(SECRET_FILE), format!("  {SECRET}  \n\n")).expect("write padded");
    assert_eq!(key::read_secret(temp.path()).expect("read padded"), BYTES);
}

/// Permission bits of `path`, without the file type bits.
#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}
