//! The topics that one bookmark carries.

use rusqlite::params;

use super::topics::{self, Topic};
use crate::{Db, Error};

/// The topics on a bookmark, ordered by name.
pub async fn for_bookmark(db: &Db, bookmark_id: i64) -> Result<Vec<Topic>, Error> {
    db.call(move |connection| topics::of_bookmark(connection, bookmark_id))
        .await
}

/// Replace the topics on one bookmark and return the new set.
///
/// Names are trimmed, matched with insensitive casing and deduplicated.
/// A topic which no longer labels this bookmark stays in the namespace.
pub async fn set(
    db: &Db,
    bookmark_id: i64,
    names: Vec<String>,
    now: i64,
) -> Result<Vec<Topic>, Error> {
    db.call(move |connection| {
        let transaction = connection.transaction()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS (SELECT 1 FROM bookmark WHERE id = ?1)",
            [bookmark_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(Error::NotFound);
        }

        let wanted = topics::resolve(&transaction, &names, now)?;

        let existing: Vec<i64> = {
            let mut statement = transaction
                .prepare("SELECT topic_id FROM bookmark_topic WHERE bookmark_id = ?1")?;
            let rows = statement.query_map([bookmark_id], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        for topic_id in &existing {
            if !wanted.iter().any(|topic| topic.id == *topic_id) {
                transaction.execute(
                    "DELETE FROM bookmark_topic WHERE bookmark_id = ?1 AND topic_id = ?2",
                    params![bookmark_id, topic_id],
                )?;
            }
        }
        for topic in &wanted {
            if !existing.contains(&topic.id) {
                transaction.execute(
                    "INSERT INTO bookmark_topic (bookmark_id, topic_id, tagged_at)
                     VALUES (?1, ?2, ?3)",
                    params![bookmark_id, topic.id, now],
                )?;
            }
        }

        let topics = topics::of_bookmark(&transaction, bookmark_id)?;
        transaction.commit()?;
        Ok(topics)
    })
    .await
}
