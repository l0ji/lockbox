//! Tests for [`lockbox_store::db`].

mod common;

use std::thread;

use common::Temp;
use lockbox_store::{Db, Error};

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

    let transaction = db
        .transaction(|_| Ok(thread::current().id()))
        .await
        .expect("transaction call");

    assert_eq!(first, second);
    assert_eq!(first, transaction);
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

#[tokio::test]
async fn commits_a_successful_transaction() {
    let temp = Temp::new("commit");
    let db = Db::open(&temp.db(), &["CREATE TABLE item (value INTEGER);"]).expect("open database");
    let value = 42;
    let inserted = db
        .transaction(move |transaction| {
            Ok(transaction.execute("INSERT INTO item VALUES (?1)", [value])?)
        })
        .await
        .expect("commit transaction");
    assert_eq!(inserted, 1);

    let stored: i64 = db
        .call(
            |connection| Ok(connection.query_row("SELECT value FROM item", [], |row| row.get(0))?),
        )
        .await
        .expect("read committed row");
    assert_eq!(stored, value);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn rolls_back_a_failed_transaction() {
    let temp = Temp::new("rollback");
    let db = Db::open(&temp.db(), &["CREATE TABLE item (value INTEGER);"]).expect("open database");
    let error = db
        .transaction::<(), _>(|transaction| {
            transaction.execute("INSERT INTO item VALUES (1)", [])?;
            Err(Error::Invalid("cancelled".to_owned()))
        })
        .await
        .expect_err("roll back transaction");
    assert!(matches!(error, Error::Invalid(message) if message == "cancelled"));

    let count: i64 = db
        .transaction(|transaction| {
            Ok(transaction.query_row("SELECT count(*) FROM item", [], |row| row.get(0))?)
        })
        .await
        .expect("run another transaction");
    assert_eq!(count, 0);
    db.close().await.expect("close database");
}

#[tokio::test]
async fn reports_a_failed_commit_and_rolls_back() {
    let temp = Temp::new("commit-error");
    let db = Db::open(
        &temp.db(),
        &["CREATE TABLE parent (id INTEGER PRIMARY KEY);
           CREATE TABLE child (
               parent_id INTEGER REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED
           );"],
    )
    .expect("open database");
    let error = db
        .transaction(|transaction| {
            transaction.execute("INSERT INTO child VALUES (1)", [])?;
            Ok(())
        })
        .await
        .expect_err("commit must fail the deferred foreign key check");
    assert!(matches!(error, Error::Invalid(_)), "{error}");

    let count: i64 = db
        .transaction(|transaction| {
            let count =
                transaction.query_row("SELECT count(*) FROM child", [], |row| row.get(0))?;
            transaction.execute("INSERT INTO parent VALUES (1)", [])?;
            transaction.execute("INSERT INTO child VALUES (1)", [])?;
            Ok(count)
        })
        .await
        .expect("commit another transaction");
    assert_eq!(count, 0);
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
