CREATE TABLE setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE own_state (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    next_seq INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE hidden (
    author    BLOB    NOT NULL,
    key       TEXT    NOT NULL,
    reason    TEXT,
    hidden_at INTEGER NOT NULL,
    PRIMARY KEY (author, key)
) WITHOUT ROWID;

CREATE TABLE blocked_author (
    author     BLOB PRIMARY KEY,
    reason     TEXT,
    blocked_at INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE own_record (
    cid        BLOB    PRIMARY KEY,
    collection TEXT    NOT NULL,
    rkey       TEXT    NOT NULL,
    target     BLOB,
    created_at INTEGER NOT NULL,
    payload    BLOB    NOT NULL,
    signature  BLOB    NOT NULL
) WITHOUT ROWID;
CREATE INDEX own_record_by_key ON own_record (collection, rkey, created_at DESC);
CREATE INDEX own_record_by_target ON own_record (target, created_at DESC)
    WHERE target IS NOT NULL;

CREATE TABLE own_head (
    collection TEXT NOT NULL,
    rkey       TEXT NOT NULL,
    cid        BLOB NOT NULL,
    PRIMARY KEY (collection, rkey)
) WITHOUT ROWID;

CREATE TABLE own_commit (
    seq          INTEGER PRIMARY KEY,
    cid          BLOB    NOT NULL UNIQUE,
    prev         BLOB,
    root         BLOB    NOT NULL,
    created_at   INTEGER NOT NULL,
    payload      BLOB    NOT NULL,
    signature    BLOB    NOT NULL,
    published_at INTEGER
) WITHOUT ROWID;
CREATE INDEX own_commit_unsent ON own_commit (seq) WHERE published_at IS NULL;
