CREATE VIRTUAL TABLE chunk_fts USING fts5(
    text,
    content = 'chunks',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TRIGGER chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunk_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
END;

CREATE TRIGGER chunks_fts_update AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunk_fts(chunk_fts, rowid, text)
    VALUES ('delete', old.rowid, old.text);
    INSERT INTO chunk_fts(rowid, text) VALUES (new.rowid, new.text);
END;

INSERT INTO chunk_fts(chunk_fts) VALUES ('rebuild');
