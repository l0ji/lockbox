//! The settings table, which holds the user's choices as text.

use std::collections::HashMap;

use rusqlite::{Connection, params};

use crate::Error;

/// Read every stored setting from `connection`.
pub fn load(connection: &Connection) -> Result<HashMap<String, String>, Error> {
    let mut stored = HashMap::new();
    let mut statement = connection.prepare("SELECT key, value FROM setting")?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        stored.insert(key, value);
    }
    Ok(stored)
}

/// Replace the stored settings in `connection`, which the caller should wrap
/// in a transaction.
pub fn store(connection: &Connection, settings: &HashMap<String, String>) -> Result<(), Error> {
    connection.execute("DELETE FROM setting", [])?;
    let mut statement = connection.prepare("INSERT INTO setting (key, value) VALUES (?1, ?2)")?;
    for (key, value) in settings {
        statement.execute(params![key, value])?;
    }
    Ok(())
}
