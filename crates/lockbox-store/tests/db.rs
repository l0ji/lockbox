//! Tests for [`lockbox_store::db`].

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU32, Ordering},
    thread,
};

use lockbox_store::{Db, Error};

/// A throwaway directory which is removed when the test ends.
struct Temp(PathBuf);

impl Temp {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("lockbox-store-{name}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create test dir");
        Self(dir)
    }

    /// Path of the database file inside the directory.
    fn db(&self) -> PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Schema version recorded in the file.
async fn version(db: &Db) -> u32 {
    db.call(|connection| Ok(connection.pragma_query_one(None, "user_version")?))
        .await
        .expect("read schema version")
}

/// Names of every table in the file.
async fn tables(db: &Db) -> Vec<String> {
    db.call(|connection| {
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")?;
        let names = statement.query_map([], |row| row.get(0))?;
        Ok(names.collect::<Result<Vec<String>, _>>()?)
    })
    .await
    .expect("list tables")
}

#[tokio::test]
async fn sets_the_pragmas_on_every_connection() {
    let temp = Temp::new("pragmas");
    let path = temp.db();
    let db = Db::open(&path, &[]).expect("open database");

    let (journal_mode, synchronous, foreign_keys, busy_timeout, temp_store) = db
        .call(|connection| {
            let journal_mode: String = connection.pragma_query_one(None, "journal_mode")?;
            let synchronous: i64 = connection.pragma_query_one(None, "synchronous")?;
            let foreign_keys: i64 = connection.pragma_query_one(None, "foreign_keys")?;
            let busy_timeout: i64 = connection.pragma_query_one(None, "busy_timeout")?;
            let temp_store: i64 = connection.pragma_query_one(None, "temp_store")?;
            Ok((
                journal_mode,
                synchronous,
                foreign_keys,
                busy_timeout,
                temp_store,
            ))
        })
        .await
        .expect("read pragmas");

    assert_eq!(journal_mode, "wal", "journal mode must be WAL");
    assert_eq!(synchronous, 1, "synchronous must be NORMAL");
    assert_eq!(foreign_keys, 1, "foreign keys must be on");
    assert_eq!(busy_timeout, 5_000, "busy timeout must be five seconds");
    assert_eq!(temp_store, 2, "temp store must be memory");
    db.close().await.expect("close database");

    // The journal mode is written into the file, so it survives a reopen
    let db = Db::open(&path, &[]).expect("reopen database");
    let journal_mode: String = db
        .call(|connection| Ok(connection.pragma_query_one(None, "journal_mode")?))
        .await
        .expect("read journal mode");
    assert_eq!(journal_mode, "wal");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn applies_migrations_in_order() {
    let temp = Temp::new("migrations");
    let path = temp.db();
    let migrations = [
        "CREATE TABLE first (value INTEGER NOT NULL);",
        "INSERT INTO first (value) VALUES (1);",
    ];

    let db = Db::open(&path, &migrations).expect("apply migrations");
    assert_eq!(version(&db).await, 2);

    let value: i64 = db
        .call(|connection| {
            Ok(connection.query_row("SELECT value FROM first", [], |row| row.get(0))?)
        })
        .await
        .expect("read the migrated row");
    assert_eq!(value, 1);
    db.close().await.expect("close database");

    // Opening again replays nothing, which would fail on the existing table
    let db = Db::open(&path, &migrations).expect("reopen database");
    assert_eq!(version(&db).await, 2);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn a_failed_migration_changes_nothing() {
    let temp = Temp::new("atomic");
    let path = temp.db();
    let migrations = [
        "CREATE TABLE kept (value INTEGER NOT NULL);",
        "CREATE TABLE lost (value INTEGER NOT NULL);
         INSERT INTO missing (value) VALUES (1);",
    ];

    let Err(error) = Db::open(&path, &migrations) else {
        panic!("the second migration must fail");
    };
    assert!(error.to_string().contains("no such table"), "{error}");

    // The file is still at version one, so the first script is not replayed
    let db = Db::open(&path, &migrations[..1]).expect("open at the previous version");
    assert_eq!(version(&db).await, 1);
    let names = tables(&db).await;
    assert!(names.iter().any(|name| name == "kept"), "{names:?}");
    assert!(!names.iter().any(|name| name == "lost"), "{names:?}");
    db.close().await.expect("close database");
}

#[tokio::test]
async fn refuses_a_file_from_a_newer_version() {
    let temp = Temp::new("newer");
    let path = temp.db();
    let migrations = [
        "CREATE TABLE one (value INTEGER);",
        "CREATE TABLE two (value INTEGER);",
    ];

    let db = Db::open(&path, &migrations).expect("apply migrations");
    db.close().await.expect("close database");

    let Err(error) = Db::open(&path, &migrations[..1]) else {
        panic!("a newer file must be refused");
    };
    match error {
        Error::TooNew { found, known } => assert_eq!((found, known), (2, 1)),
        other => panic!("expected a version error, got {other}"),
    }

    // Nothing was touched, so the file still opens at its own version
    let db = Db::open(&path, &migrations).expect("open at the file's version");
    assert_eq!(version(&db).await, 2);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn calls_run_on_one_thread() {
    let temp = Temp::new("thread");
    let db = Db::open(&temp.db(), &[]).expect("open database");

    let first = db
        .call(|_| Ok(thread::current().id()))
        .await
        .expect("first call");
    let second = db
        .call(|_| Ok(thread::current().id()))
        .await
        .expect("second call");

    assert_eq!(first, second);
    assert_ne!(first, thread::current().id());
    db.close().await.expect("close database");
}

#[tokio::test]
async fn an_error_does_not_stop_the_thread() {
    let temp = Temp::new("error");
    let migrations = ["CREATE TABLE kept (value INTEGER);"];
    let db = Db::open(&temp.db(), &migrations).expect("open database");

    let error = db
        .call(|connection| Ok(connection.execute_batch("SELECT * FROM missing")?))
        .await
        .expect_err("the statement must fail");
    assert!(error.to_string().contains("no such table"), "{error}");

    let rows: i64 = db
        .call(|connection| {
            Ok(connection.query_row("SELECT count(*) FROM kept", [], |row| row.get(0))?)
        })
        .await
        .expect("the database still answers");
    assert_eq!(rows, 0);
    db.close().await.expect("close database");
}

#[cfg(unix)]
#[tokio::test]
async fn creates_the_file_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let temp = Temp::new("mode");
    let path = temp.db();
    let db = Db::open(&path, &[]).expect("open database");

    let mode = std::fs::metadata(&path)
        .expect("stat database")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    db.close().await.expect("close database");
}
