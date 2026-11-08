CREATE VIRTUAL TABLE entry_search USING fts5(
    title, summary, body,
    content = 'entry', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER entry_search_ai AFTER INSERT ON entry BEGIN
    INSERT INTO entry_search (rowid, title, summary, body)
    VALUES (new.id, new.title, new.summary, new.body);
END;

CREATE TRIGGER entry_search_ad AFTER DELETE ON entry BEGIN
    INSERT INTO entry_search (entry_search, rowid, title, summary, body)
    VALUES ('delete', old.id, old.title, old.summary, old.body);
END;

CREATE TRIGGER entry_search_au AFTER UPDATE OF title, summary, body ON entry BEGIN
    INSERT INTO entry_search (entry_search, rowid, title, summary, body)
    VALUES ('delete', old.id, old.title, old.summary, old.body);
    INSERT INTO entry_search (rowid, title, summary, body)
    VALUES (new.id, new.title, new.summary, new.body);
END;

-- Entries stored before the search table existed
INSERT INTO entry_search (entry_search) VALUES ('rebuild');
