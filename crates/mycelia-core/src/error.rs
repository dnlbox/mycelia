use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("corpus root does not exist or is not a directory: {0}")]
    InvalidRoot(PathBuf),
    #[error("database belongs to corpus {expected}, not {actual}")]
    CorpusMismatch { expected: String, actual: String },
    #[error("query must not be empty")]
    EmptyQuery,
    #[error("query contains no searchable tokens")]
    NoSearchTerms,
    #[error("result limit must be greater than zero")]
    InvalidLimit,
    #[error("evaluation case has no expected matches: {0}")]
    EvaluationCaseWithoutExpected(String),
    #[error("embedding provider failed: {0}")]
    EmbeddingProvider(String),
    #[error("embedding dimensions differ: expected {expected}, found {found}")]
    EmbeddingDimensions { expected: usize, found: usize },
    #[error("embedding vector contains a non-finite value")]
    NonFiniteEmbedding,
    #[error("no cached embeddings for model {0}; run `mycelia embed` first")]
    MissingEmbeddings(String),
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchemaVersion { found: i64, supported: i64 },
    #[error("path is outside the corpus root: {0}")]
    PathOutsideRoot(PathBuf),
    #[error("index has no recorded corpus root; re-run `mycelia index`")]
    MissingCorpusRoot,
    #[error("path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
