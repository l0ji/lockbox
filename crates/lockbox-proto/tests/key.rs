//! Tests for [`lockbox_proto::key`].

use lockbox_proto::key::{self, KEY_LEN, KeyError, PUBLIC_PREFIX, SECRET_PREFIX};

/// Bytes 0-31, which encode to the vectors below.
const BYTES: [u8; KEY_LEN] = [
    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
    0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
];

const PUBLIC: &str = "lbox1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0ssqs23x";
const SECRET: &str = "lsec1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0s3p0mx6";

#[test]
fn encodes_a_public_key() {
    assert_eq!(key::encode_public(&BYTES), PUBLIC);
    assert_eq!(PUBLIC.len(), 63);
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
        "lbox1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqe5mg3l"
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
    let broken = "lbox1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0ssqs23q";
    assert_eq!(key::decode_public(broken), Err(KeyError::Encoding));
}

#[test]
fn refuses_a_plain_bech32_checksum() {
    let bech32 = "lbox1qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0s9uqx5y";
    assert_eq!(key::decode_public(bech32), Err(KeyError::Encoding));
}

#[test]
fn refuses_upper_case() {
    let upper = "LBOX1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SSQS23X";
    assert_eq!(key::decode_public(upper), Err(KeyError::Encoding));
}

#[test]
fn refuses_a_payload_that_is_not_a_key() {
    let short = "lbox1qqqsyqcyq5rqwzqfpg9scrgwpu8ltxjk";
    assert_eq!(key::decode_public(short), Err(KeyError::Length));
}

#[test]
fn refuses_plain_text() {
    assert_eq!(key::decode_public("not a key"), Err(KeyError::Encoding));
    assert_eq!(key::decode_secret(""), Err(KeyError::Encoding));
}

#[test]
fn fingerprints_the_start_of_the_public_key() {
    let fingerprint = key::fingerprint(&BYTES);
    assert_eq!(fingerprint, "qqqsy-qcyq5");
    assert_eq!(key::fingerprint(&[0; KEY_LEN]), "qqqqq-qqqqq");

    // The digits are the data part of the public key, without the separator
    let data = &PUBLIC[PUBLIC_PREFIX.len() + 1..];
    assert_eq!(fingerprint.replace('-', ""), data[..key::FINGERPRINT_LEN]);
}

#[test]
fn fingerprints_are_grouped_in_twos() {
    let fingerprint = key::fingerprint(&BYTES);
    assert_eq!(fingerprint.len(), key::FINGERPRINT_LEN + 1);
    let (head, tail) = fingerprint.split_once('-').expect("two groups");
    assert_eq!(head.len(), tail.len());
    assert!(head.chars().all(|c| c.is_ascii_alphanumeric()));
}

#[test]
fn the_fingerprint_covers_the_start_of_the_key() {
    let mut other = BYTES;
    other[0] = 0xff;
    assert_ne!(key::fingerprint(&BYTES), key::fingerprint(&other));

    // It is a prefix, so a difference at the end of the key is invisible to it
    let mut late = BYTES;
    late[KEY_LEN - 1] = 0xff;
    assert_eq!(key::fingerprint(&BYTES), key::fingerprint(&late));
}
