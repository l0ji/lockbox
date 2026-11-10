//! Shared protocol implementation for lockbox.
//!
//! A few constants live here. The p2p protocol is locked by [`ALPN`] and
//! [`PROTOCOL_VERSION`]. The local HTTP server ran by a server implementation
//! which clients consume is in [`api`]. Both binaries
//! determine where the socket is stored from [`local`]. Account keys are
//! defined in [`key`] and the names of their files in [`config`].
//!
//! This crate should cover purely the technical specification of the protocol
//! and any asynchronous runtimes, network stacks or other side effect-producing
//! code should be stored somewhere else.

pub mod api;
pub mod config;
pub mod key;
pub mod local;

/// ALPN protocol name negotiated on every lockbox connection.
pub const ALPN: &[u8] = b"lockbox/1";

/// Current version of the lockbox wire protocol.
pub const PROTOCOL_VERSION: u16 = 1;
