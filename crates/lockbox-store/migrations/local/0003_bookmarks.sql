CREATE TABLE site (
    url          TEXT PRIMARY KEY,
    title        TEXT,
    description  TEXT,
    image        TEXT,
    feed_url     TEXT,
    lang         TEXT,
    latest_item_at INTEGER,
    fetched_at   INTEGER,
    rkey         TEXT,
    record_cid   BLOB,
    published_at INTEGER,
    withdrawn_at INTEGER
) WITHOUT ROWID;
CREATE INDEX site_by_feed ON site (feed_url) WHERE feed_url IS NOT NULL;

CREATE TABLE folder (
    id         INTEGER PRIMARY KEY,
    parent_id  INTEGER REFERENCES folder (id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    kind       TEXT    NOT NULL DEFAULT 'manual'
               CHECK (kind IN ('manual', 'dynamic')),
    position   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX folder_root_name ON folder (name) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX folder_child_name ON folder (parent_id, name)
    WHERE parent_id IS NOT NULL;

CREATE TABLE bookmark (
    id            INTEGER PRIMARY KEY,
    url           TEXT    NOT NULL,
    canonical_url TEXT    NOT NULL,
    title         TEXT    NOT NULL,
    note          TEXT,
    folder_id     INTEGER REFERENCES folder (id) ON DELETE SET NULL,
    position      INTEGER NOT NULL DEFAULT 0,
    entry_id      INTEGER REFERENCES entry (id) ON DELETE SET NULL,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL,
    archived_at   INTEGER,
    visibility    TEXT    NOT NULL DEFAULT 'public'
                  CHECK (visibility IN ('public', 'private')),
    rkey          TEXT,
    record_cid    BLOB,
    published_at  INTEGER,
    withdrawn_at  INTEGER
);
CREATE UNIQUE INDEX bookmark_by_url ON bookmark (canonical_url);
CREATE INDEX bookmark_by_folder ON bookmark (folder_id, position, id);
CREATE INDEX bookmark_by_created ON bookmark (created_at DESC, id DESC);
CREATE INDEX bookmark_by_published ON bookmark (record_cid)
    WHERE record_cid IS NOT NULL;

CREATE TABLE topic (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    created_at   INTEGER NOT NULL,
    followed_at  INTEGER,
    rkey         TEXT,
    record_cid   BLOB,
    published_at INTEGER,
    withdrawn_at INTEGER
);

CREATE TABLE bookmark_topic (
    bookmark_id INTEGER NOT NULL REFERENCES bookmark (id) ON DELETE CASCADE,
    topic_id    INTEGER NOT NULL REFERENCES topic (id) ON DELETE CASCADE,
    tagged_at   INTEGER NOT NULL,
    record_cid  BLOB
);
CREATE UNIQUE INDEX bookmark_topic_once ON bookmark_topic (bookmark_id, topic_id);
CREATE INDEX bookmark_topic_by_topic ON bookmark_topic (topic_id, bookmark_id);

CREATE TABLE folder_topic (
    folder_id INTEGER NOT NULL REFERENCES folder (id) ON DELETE CASCADE,
    topic_id  INTEGER NOT NULL REFERENCES topic (id) ON DELETE CASCADE,
    PRIMARY KEY (folder_id, topic_id)
) WITHOUT ROWID;
CREATE INDEX folder_topic_by_topic ON folder_topic (topic_id, folder_id);
