//! Data persistence for lockbox, spread across two SQLite files for local and
//! global views.
//!
//! [`db`] holds the connection thread, the connection pragmas and the migration
//! runner. Every query goes through [`db::Db::call`].

pub mod db;
pub mod local;

pub use db::{Db, Error};
