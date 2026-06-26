CREATE TABLE IF NOT EXISTS metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sources (
    path TEXT PRIMARY KEY NOT NULL,
    content_hash TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    id TEXT PRIMARY KEY NOT NULL,
    source_path TEXT NOT NULL REFERENCES sources(path) ON DELETE CASCADE,
    source_hash TEXT NOT NULL,
    byte_start INTEGER NOT NULL,
    byte_end INTEGER NOT NULL,
    line_start INTEGER NOT NULL,
    line_end INTEGER NOT NULL,
    text TEXT NOT NULL,
    extractor TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS chunks_source_path_idx ON chunks(source_path);
