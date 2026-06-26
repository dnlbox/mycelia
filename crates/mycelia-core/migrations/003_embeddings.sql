CREATE TABLE embeddings (
    chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL,
    dimensions INTEGER NOT NULL,
    vector BLOB NOT NULL,
    PRIMARY KEY(chunk_id, model_id)
);

CREATE INDEX embeddings_model_id_idx ON embeddings(model_id);
