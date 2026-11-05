# Keypairs

A *keypair* consists of a secret key and its public key with a length of 32
octets. The keypair is used primarily as the identity system on lockbox.

## Key Encoding

A key of either half is encoded as a bech32m string whose human-readable part
(HRP) identifies the half:

| Half       | HRP     | Example                                                            |
|------------|---------|--------------------------------------------------------------------|
| Public key | `lbox0` | `lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sw7hefe` |
| Secret key | `lsec0` | `lsec01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sjhjmgt` |

The data part is the 32 octets of the key, converted to 5-bit groups as
specified in [BIP-350]. No other payload length is valid. Encoded strings are
64 characters long: the HRP, the `1` separator, 52 data characters, and six
checksum characters.

Encoders MUST emit lower case. Decoders MUST reject upper case, MUST reject a
string that carries the bech32 checksum instead of the bech32m one, MUST reject
a payload that is not exactly 32 octets, and MUST reject a trailing pad that is
not zero. A decoder for one half MUST reject the HRP of the other half.

The canonical form of a string is the encoding of the octets it decodes to. A
decoder that rejects every string that differs from its canonical form enforces
all of the above at once.

## Endpoint Identifier

The 32 unencoded octets of the public key are used as the endpoint identifier
of the node. No transformation, truncation, or additional hash is applied. The
identifier is immutable for the lifetime of a keypair.

The `lbox0` string is the presentation form of the public key and is the form
used in user-facing text. The local API reports the identifier in lower-case
hexadecimal, as `Status.node_id`.

### Fingerprint

A fingerprint is the first ten characters of the data part of the `lbox0`
encoding, in two groups of five separated by a hyphen. It is eleven characters
long and carries no checksum.

Fingerprints are for reading a key out loud and for comparing two keys by eye. A
fingerprint MUST NOT be used as an identifier or as a storage key, and a
comparison that decides which key a party holds MUST use the full encoding.

Only the leading 50 bits of the public key reach the fingerprint, so two keys
that differ beyond that prefix share one.

For the first vector of the test vectors, the fingerprint is `qqqsy-qcyq5`.

## Keypair Storage

The keypair is stored as two files in a single directory associated to the protocol:

| File         | Content              | Permission bits (UNIX) |
|--------------|----------------------|------------------------|
| `secret.key` | The `lsec0` string   | `0600`                 |
| `public.key` | The `lbox0` string   | `0644`                 |

Each file holds one encoded string followed by a newline. Decoders MUST ignore
leading and trailing whitespace and MUST reject any input that decodes to more
than one string.

Both files are created exclusively; an existing file MUST NOT be replaced.
`public.key` is a copy for other tools and MUST NOT be used to establish the
identity of a node. The secret key is authoritative.

### Directory Resolution

The containing directory is resolved in the following order, where an
implementation MUST take the first entry that is available:

1. the directory named by the `LOCKBOX_DIR` environment variable, when it is set
   and not empty;
2. `/etc/lockbox`, when that directory exists. A keypair installed there by an
   administrator is read even when it cannot be written by the daemon;
3. `lockbox` under `XDG_CONFIG_HOME` [XDG], or under `HOME/.config` when
   `XDG_CONFIG_HOME` is unset. The directory is created when it is missing;
4. `.lockbox` under `HOME`, when entry 3 cannot be created or written;
5. otherwise the implementation MUST fail and name `LOCKBOX_DIR`.

An implementation MAY offer a mode in which entry 2 is skipped, so that a user
keypair is used even when a system-wide one exists. The `lockboxd` daemon
selects it with `--user-keys`.

## Keypair Generation

On startup an implementation reads `secret.key` from the resolved directory:

- when the file is missing, the implementation generates a new keypair, writes
  both files, and reports the public key together with the path of `secret.key`;
- when the file is present but does not decode as a `lsec0` string, the
  implementation MUST fail. A keypair MUST NOT be generated in that case;
- when `secret.key` is present and `public.key` is missing, the implementation
  writes `public.key` again from the secret key.

After generation, a newly generated keypair SHOULD be reported with a warning
that names the file, SHOULD state the public key and its fingerprint, and MUST
direct the user to back up the secret key and to keep it private.

## Security measures

The secret key authenticates the node and is sufficient to impersonate it. Any
party that holds `secret.key` controls the account.

The secret key MUST NOT appear in logs, in error messages, or in any message
sent over the local API or the network protocol. The public key is not secret
and MAY be logged and shared freely.

Writers SHOULD create the key directory with owner-only permissions and MUST
create `secret.key` with `0600` at creation time rather than by a subsequent
change of permission bits.

The bech32m checksum detects transcription errors. It does not authenticate the
key, and a valid encoding carries no statement about who holds the secret key.

This document defines no key rotation or revocation procedure.

## Test vectors

The octets `00 01 02 03 04 05 06 07 08 09 0a 0b 0c 0d 0e 0f 10 11 12 13 14 15 16
17 18 19 1a 1b 1c 1d 1e 1f`:

```txt
lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sw7hefe
lsec01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0sjhjmgt
```

Thirty-two zero octets:

```txt
lbox01qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq82umfq
```

The same thirty-two octets under the bech32 checksum, which a decoder MUST
reject:

```txt
lbox01qqqsyqcyq5rqwzqfpg9scrgwpugpzysnzs23v9ccrydpk8qarc0smz84vm
```

## References

### Normative

- [BIP-173] Friedenbach, M. et al., "Base32 address format for native v0-16
  witness program outputs", BIP 173.
- [BIP-350] Somsen, P. and M. Kuhn, "Bech32m format for v1+ witness addresses",
  BIP 350.
- [XDG] "XDG Base Directory Specification", freedesktop.org.

### Informative

- [IROH] "iroh", <https://iroh.computer>, the transport that carries the lockbox
  protocol and defines the endpoint identifier that uses the public key.
