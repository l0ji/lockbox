//! Data types and structs which the crates of this project share.

pub mod entry;
pub mod error;
pub mod feed;
pub mod node;

mod time;

pub use error::root_cause;
pub use time::now;

use serde::{Deserialize, Deserializer};

/// Read a field which may be left out, or sent as a `null` to clear it.
pub fn nullable<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}
