CREATE TABLE feed (
    id              INTEGER PRIMARY KEY,
    url             TEXT    NOT NULL UNIQUE,
    site_url        TEXT,
    title           TEXT,
    kind            TEXT    NOT NULL DEFAULT 'rss' CHECK (kind IN ('rss', 'atom', 'json')),
    poll_seconds    INTEGER NOT NULL DEFAULT 3600 CHECK (poll_seconds >= 60),
    next_poll_at    INTEGER,
    fetched_at      INTEGER,
    etag            TEXT,
    last_modified   TEXT,
    last_error      TEXT,
    visibility      TEXT    NOT NULL DEFAULT 'private'
                    CHECK (visibility IN ('public', 'private')),
    disabled        INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    discovered_from TEXT    NOT NULL DEFAULT 'user'
                    CHECK (discovered_from IN ('user', 'page', 'federation')),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
);

CREATE TABLE entry (
    id            INTEGER PRIMARY KEY,
    feed_id       INTEGER NOT NULL REFERENCES feed (id) ON DELETE CASCADE,
    guid          TEXT    NOT NULL UNIQUE,
    source        TEXT    NOT NULL,
    url           TEXT,
    canonical_url TEXT,
    title         TEXT    NOT NULL,
    author        TEXT,
    summary       TEXT,
    body          TEXT,
    truncated     INTEGER NOT NULL DEFAULT 0 CHECK (truncated IN (0, 1)),
    published_at  INTEGER,
    sort_at       INTEGER NOT NULL,
    fetched_at    INTEGER NOT NULL,
    read_at       INTEGER,
    starred_at    INTEGER,
    archived_at   INTEGER,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    UNIQUE (feed_id, source)
);
CREATE INDEX entry_by_sort ON entry (sort_at DESC, id DESC);
CREATE INDEX entry_by_feed ON entry (feed_id, sort_at DESC, id DESC);
CREATE INDEX entry_by_unread ON entry (sort_at DESC, id DESC)
    WHERE read_at IS NULL AND archived_at IS NULL;
CREATE INDEX entry_by_starred ON entry (starred_at DESC, id DESC)
    WHERE starred_at IS NOT NULL;
