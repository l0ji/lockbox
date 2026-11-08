CREATE VIRTUAL TABLE bookmark_search USING fts5(
    title, note, url,
    content = 'bookmark', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER bookmark_search_ai AFTER INSERT ON bookmark BEGIN
    INSERT INTO bookmark_search (rowid, title, note, url)
    VALUES (new.id, new.title, new.note, new.url);
END;

CREATE TRIGGER bookmark_search_ad AFTER DELETE ON bookmark BEGIN
    INSERT INTO bookmark_search (bookmark_search, rowid, title, note, url)
    VALUES ('delete', old.id, old.title, old.note, old.url);
END;

CREATE TRIGGER bookmark_search_au AFTER UPDATE OF title, note, url ON bookmark BEGIN
    INSERT INTO bookmark_search (bookmark_search, rowid, title, note, url)
    VALUES ('delete', old.id, old.title, old.note, old.url);
    INSERT INTO bookmark_search (rowid, title, note, url)
    VALUES (new.id, new.title, new.note, new.url);
END;

-- Bookmarks stored before the search table existed
INSERT INTO bookmark_search (bookmark_search) VALUES ('rebuild');
