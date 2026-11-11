//! Data persistence for lockbox, spread across two SQLite files for local and
//! global views.
//!
//! [`db`] holds the connection thread, the connection pragmas and the migration
//! runner, and every query goes through [`db::Db::call`]. The rows the queries
//! read and write are the model types in `lockbox-types`.

pub mod db;
pub mod local;

pub use db::{Db, Error};
