//! The database connection thread, its pragmas and the migrations.
//!
//! A [`Db`] owns one [`Connection`] on a thread of its own. [`Db::call`] is the
//! only way to touch it, so blocking SQLite work never runs on a runtime worker.

use std::{fmt, io, path::Path, thread};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

/// Number of calls that may wait for the database thread.
const QUEUE: usize = 64;

/// Error type used for the store.
#[derive(Debug)]
pub enum Error {
    /// SQLite returned an error.
    Sqlite(rusqlite::Error),
    /// A file system operation failed.
    Io(io::Error),
    /// The database file was written by a newer version of lockbox.
    TooNew { found: u32, known: u32 },
    /// Could not find the row requested by the call.
    NotFound,
    /// The database thread was closed.
    Closed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "{error}"),
            Self::Io(error) => write!(f, "{error}"),
            Self::TooNew { found, known } => {
                write!(f, "database version {found}, expected version {known}")
            }
            Self::NotFound => write!(f, "row not found"),
            Self::Closed => write!(f, "database thread is gone"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::TooNew { .. } | Self::NotFound | Self::Closed => None,
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// A call queued for the database thread.
type Call = Box<dyn FnOnce(&mut Connection) + Send>;

/// An abstraction over the database thread.
///
/// Dropping the handle stops the thread once its queue is empty. [`Db::close`]
/// waits for that to finish.
pub struct Db {
    calls: mpsc::Sender<Call>,
    stopped: oneshot::Receiver<()>,
}

impl Db {
    /// Open the database at `path`, applying every migration it has not seen.
    ///
    /// A file which is at a newer version than `migrations` will not open.
    pub fn open(path: &Path, migrations: &[&str]) -> Result<Self, Error> {
        #[cfg(unix)]
        let created = !path.try_exists()?;

        let mut connection = Connection::open(path)?;

        #[cfg(unix)]
        if created {
            // SQLite applies the process umask, which is usually wider than 0600
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }

        configure(&connection)?;
        migrate(&mut connection, migrations)?;

        let (calls, mut queue) = mpsc::channel::<Call>(QUEUE);
        let (finished, stopped) = oneshot::channel();
        thread::Builder::new()
            .name("lockbox-store".to_owned())
            .spawn(move || {
                while let Some(call) = queue.blocking_recv() {
                    call(&mut connection);
                }
                // Give the query planner a chance to update its statistics
                let _ = connection.execute_batch("PRAGMA optimize");
                let _ = finished.send(());
            })?;

        Ok(Self { calls, stopped })
    }

    /// Run `call` on the database thread and await the result.
    pub async fn call<T, F>(&self, call: F) -> Result<T, Error>
    where
        F: FnOnce(&mut Connection) -> Result<T, Error> + Send + 'static,
        T: Send + 'static,
    {
        let (reply, answer) = oneshot::channel();
        let request: Call = Box::new(move |connection| {
            let _ = reply.send(call(connection));
        });
        self.calls.send(request).await.map_err(|_| Error::Closed)?;

        match answer.await {
            Ok(result) => result,
            Err(_) => Err(Error::Closed),
        }
    }

    /// Stop the database thread and wait for it to close the connection.
    pub async fn close(self) -> Result<(), Error> {
        let Self { calls, stopped } = self;
        drop(calls);
        stopped.await.map_err(|_| Error::Closed)
    }
}

/// Set the connection pragmas that every connection needs.
fn configure(connection: &Connection) -> Result<(), Error> {
    // WAL is persisted in the file, the rest are per connection
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    Ok(())
}

/// Apply every migration which the file does not have applied, one transaction
/// at a time.
fn migrate(connection: &mut Connection, migrations: &[&str]) -> Result<(), Error> {
    let known = migrations.len() as u32;
    let applied = version(connection)?;
    if applied > known {
        return Err(Error::TooNew {
            found: applied,
            known,
        });
    }

    for (position, script) in migrations.iter().enumerate().skip(applied as usize) {
        let transaction = connection.transaction()?;
        transaction.execute_batch(script)?;
        transaction.pragma_update(None, "user_version", position as u32 + 1)?;
        transaction.commit()?;
    }
    Ok(())
}

/// The database's current migration version.
fn version(connection: &Connection) -> Result<u32, Error> {
    Ok(connection.pragma_query_one(None, "user_version")?)
}
