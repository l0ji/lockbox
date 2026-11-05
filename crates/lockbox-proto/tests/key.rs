//! Tests for [`lockbox_proto::key`].

use lockbox_proto::key::{self, KEY_LEN, KeyError, PUBLIC_PREFIX, SECRET_PREFIX};

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
