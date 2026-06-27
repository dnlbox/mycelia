-- Symbol identity for code chunks: the defined name (function, struct, class,
-- ...). NULL for plain-text chunks and for code chunks indexed before this
-- migration; a `mycelia refresh` repopulates them. Indexed for name resolution.
ALTER TABLE chunks ADD COLUMN symbol TEXT;

CREATE INDEX IF NOT EXISTS chunks_symbol_idx ON chunks(symbol);

-- Typed edges between chunks, stored by callee NAME (`dst_symbol`), not a
-- resolved target id. Resolution to a defining chunk happens at query time
-- against the current symbol index, so edges stay correct under incremental and
-- partial reindex. `confidence` records the extraction-time class (EXTRACTED for
-- deterministic tree-sitter edges); query-time ambiguity is computed, not stored.
-- The provenance span is the call site inside the source chunk. Edges cascade
-- with their owning chunk: a reindex deletes the chunk row and its outgoing edges.
CREATE TABLE IF NOT EXISTS edges (
    src_chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    edge_type    TEXT NOT NULL,
    dst_symbol   TEXT NOT NULL,
    confidence   TEXT NOT NULL,
    byte_start   INTEGER NOT NULL,
    byte_end     INTEGER NOT NULL,
    line_start   INTEGER NOT NULL,
    line_end     INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS edges_src_idx ON edges(src_chunk_id);
CREATE INDEX IF NOT EXISTS edges_dst_idx ON edges(dst_symbol);
