use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mycelia_core::{RetrievalStrategy, Retrieved, SearchHeader};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::Deserialize;

use crate::log::CorpusLogger;
use crate::semantic::{FastEmbedProvider, MODEL_ID};

type McpToolResult<T> = std::result::Result<T, String>;

/// The embedding provider shared across `find` calls on one server. Tool
/// handlers take `&self`, and `embed_query` needs `&mut`, so the provider lives
/// behind a `Mutex`; `Arc` keeps `MyceliaServer` cloneable for the tool router.
type SharedProvider = Arc<Mutex<FastEmbedProvider>>;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindRequest {
    #[schemars(description = "Text to search for in the configured Mycelia index")]
    query: String,
    #[serde(default = "default_limit")]
    #[schemars(description = "Maximum number of sourced headers to return")]
    limit: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RetrieveRequest {
    #[schemars(description = "Deterministic chunk identifier returned by Mycelia")]
    chunk_id: String,
}

const SERVER_INSTRUCTIONS: &str = "Mycelia is the token-efficient orientation path for this bound corpus. For questions like where something is implemented, what supports a feature or language, which files touch an area, or which symbol defines behavior, call find first. Use retrieve only for the selected chunk ids that look relevant. Shell grep/read remain useful follow-up tools for exact line edits after Mycelia has narrowed the search.";

#[derive(Clone)]
struct MyceliaServer {
    database: PathBuf,
    /// `Some` routes queries through embeddings; `None` serves lexical-only.
    provider: Option<SharedProvider>,
    logger: Option<CorpusLogger>,
    tool_router: ToolRouter<Self>,
}

impl MyceliaServer {
    fn new(
        database: PathBuf,
        provider: Option<SharedProvider>,
        logger: Option<CorpusLogger>,
    ) -> Self {
        Self {
            database,
            provider,
            logger,
            tool_router: Self::tool_router(),
        }
    }

    /// Runs the bound retrieval path for one query and projects to headers.
    /// Routed when a provider is loaded (it already falls back to reranked FTS5
    /// for chunks lacking embeddings, including freshly re-indexed ones), lexical
    /// otherwise.
    fn find_headers(&self, query: &str, limit: usize) -> McpToolResult<Vec<SearchHeader>> {
        match &self.provider {
            Some(provider) => {
                let mut guard = provider.lock().map_err(|error| error.to_string())?;
                mycelia_core::find_headers_with_embeddings(
                    &self.database,
                    query,
                    limit,
                    RetrievalStrategy::Routed,
                    &mut *guard,
                )
            }
            None => mycelia_core::find_headers(&self.database, query, limit),
        }
        .map_err(|error| error.to_string())
    }

    fn search_json(&self, request: &FindRequest) -> McpToolResult<String> {
        let headers = self.find_headers(&request.query, request.limit)?;

        // Validate the sources behind the returned headers against disk. If any
        // drifted, self-heal them and re-rank once so the headers stay precise
        // and cannot contradict a later retrieve. The filesystem is the single
        // source of truth, so find and retrieve converge on it. Headers carry no
        // stale flag: a re-ranked result is simply accurate.
        let paths: Vec<String> = headers
            .iter()
            .map(|header| header.source_path.clone())
            .collect();
        let drifted = mycelia_core::drifted_sources(&self.database, &paths)
            .map_err(|error| error.to_string())?;
        let headers = if drifted.is_empty() {
            headers
        } else {
            for path in &drifted {
                // Internal maintenance of the bound index; a heal error must not
                // fail the search. The worst case is a slightly stale header that
                // retrieve still corrects.
                let _ = mycelia_core::refresh_source(&self.database, path);
            }
            self.find_headers(&request.query, request.limit)?
        };

        if let Some(logger) = &self.logger {
            logger.log_find(&request.query, &headers);
        }

        serde_json::to_string(&headers).map_err(|error| error.to_string())
    }
}

#[tool_router]
impl MyceliaServer {
    #[tool(
        description = "Cheap first-pass codebase orientation over the configured Mycelia index. Use before grep/read when locating implementations, supported features, related files, symbols, or concepts in this corpus. Returns ranked source headers with paths, line ranges, signatures or synopses, scores, and chunk ids without opening full files."
    )]
    fn find(&self, Parameters(request): Parameters<FindRequest>) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Alias for find with a more explicit name: search the indexed codebase before grep/read to cheaply locate relevant files, symbols, features, or implementation areas."
    )]
    fn search_codebase(
        &self,
        Parameters(request): Parameters<FindRequest>,
    ) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Alias for find tuned for implementation hunts. Use for questions like where is X implemented, what supports Y, or which source chunks define a feature before opening raw files."
    )]
    fn locate_implementation(
        &self,
        Parameters(request): Parameters<FindRequest>,
    ) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Fetch the exact body for one chunk id selected from a prior Mycelia search result. Use after find/search_codebase/locate_implementation has identified the specific source chunk worth reading."
    )]
    fn retrieve(&self, Parameters(request): Parameters<RetrieveRequest>) -> McpToolResult<String> {
        // Fresh sources return the precise chunk; a changed source returns the
        // whole current file; a gone/unreadable source returns an `unavailable`
        // signal. The model is never handed an indexed chunk whose source moved.
        // An unknown identifier is still a hard error.
        let outcome = mycelia_core::retrieve(&self.database, &request.chunk_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("chunk not found: {}", request.chunk_id))?;

        if let Some(logger) = &self.logger {
            let source_path = match &outcome {
                mycelia_core::Retrieved::Ok { chunk } => chunk.source_path.as_str(),
                mycelia_core::Retrieved::File { source_path, .. } => source_path.as_str(),
                mycelia_core::Retrieved::Unavailable { source_path, .. } => source_path.as_str(),
            };
            logger.log_retrieve(&request.chunk_id, source_path);
        }

        let mut value = serde_json::to_value(&outcome).map_err(|error| error.to_string())?;

        // Drift was detected. Silently self-heal the launch-bound index so later
        // queries read fresh data; this is internal maintenance, not a
        // model-facing write. Never fail the call on a heal error: the caller
        // already has correct content. Only when the index cannot be repaired
        // (for example a read-only filesystem) surface a last-resort hint the
        // harness can relay so the user can run `mycelia refresh`.
        if let Some(source_path) = drifted_source(&outcome)
            && mycelia_core::refresh_source(&self.database, source_path).is_err()
            && let Some(object) = value.as_object_mut()
        {
            object.insert(
                "refresh_hint".to_owned(),
                serde_json::Value::String(
                    "index self-update failed; run `mycelia refresh` when convenient".to_owned(),
                ),
            );
        }

        serde_json::to_string(&value).map_err(|error| error.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MyceliaServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("mycelia", env!("CARGO_PKG_VERSION"))
                    .with_title("Mycelia local knowledge index"),
            )
            .with_instructions(SERVER_INSTRUCTIONS)
    }
}

pub(crate) fn serve(
    database: PathBuf,
    corpus_name: Option<String>,
    corpus_root: Option<PathBuf>,
    lexical: bool,
) -> McpToolResult<()> {
    if !database.is_file() {
        return Err(format!(
            "MCP database does not exist or is not a file: {}",
            database.display()
        ));
    }

    // Build the logger before loading the model so the serve-start event names
    // the embeddings status accurately. Best-effort: a missing log directory
    // just means no log.
    let logger = corpus_name.as_deref().and_then(|name| {
        crate::profile::log_path_for(name)
            .ok()
            .map(|path| CorpusLogger::open(path, corpus_root.clone()))
    });

    // Load the embedding model once at startup so `find` can route. Diagnostics
    // go to stderr; stdout is reserved for the MCP protocol. The model is loaded
    // only when the bound corpus actually has embeddings, so serving an
    // unembedded index never pays model init or triggers a download. A load
    // failure (for example, an air-gapped first run) degrades to lexical
    // retrieval rather than refusing to serve.
    let provider = if lexical {
        None
    } else if !mycelia_core::has_embeddings(&database, MODEL_ID)
        .map_err(|error| error.to_string())?
    {
        eprintln!("mycelia: corpus has no embeddings, serving reranked FTS5");
        None
    } else {
        match FastEmbedProvider::load(&database) {
            Ok(provider) => Some(Arc::new(Mutex::new(provider))),
            Err(error) => {
                eprintln!(
                    "mycelia: embedding model unavailable, serving lexical retrieval: {error}"
                );
                None
            }
        }
    };

    let embeddings_status = if provider.is_some() {
        "routed"
    } else {
        "lexical"
    };
    if let Some(logger) = &logger {
        logger.log_serve_start(MODEL_ID, embeddings_status);
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;

    runtime.block_on(async move {
        let service = MyceliaServer::new(database, provider, logger)
            .serve(stdio())
            .await
            .map_err(|error| error.to_string())?;
        service.waiting().await.map_err(|error| error.to_string())?;
        Ok(())
    })
}

fn default_limit() -> usize {
    10
}

/// The source path when `retrieve` detected drift (a changed or vanished
/// source), signalling that the bound index is worth self-healing. A fresh,
/// precise chunk needs no repair.
fn drifted_source(outcome: &Retrieved) -> Option<&str> {
    match outcome {
        Retrieved::File { source_path, .. } | Retrieved::Unavailable { source_path, .. } => {
            Some(source_path.as_str())
        }
        Retrieved::Ok { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn find_returns_sourced_headers_from_bound_database() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("notes.txt"), "a precise sourced result").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let server = MyceliaServer::new(database, None, None);

        let output = server
            .find(Parameters(FindRequest {
                query: "precise".to_owned(),
                limit: 5,
            }))
            .expect("find");

        assert!(output.contains("\"source_path\":\"notes.txt\""));
        assert!(output.contains("\"chunk_id\""));
        assert!(!output.contains("\"text\""));
    }

    #[test]
    fn retrieve_reports_missing_chunk() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("notes.txt"), "indexed content").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let server = MyceliaServer::new(database, None, None);

        let error = server
            .retrieve(Parameters(RetrieveRequest {
                chunk_id: "missing".to_owned(),
            }))
            .expect_err("missing chunk");

        assert_eq!(error, "chunk not found: missing");
    }

    #[test]
    fn retrieve_returns_live_file_after_source_changes() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("notes.txt");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(&file, "the original sourced content").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let chunk_id = mycelia_core::find(&database, "original", 5).expect("find")[0]
            .chunk
            .id
            .clone();

        // Mutate the source without re-indexing; the indexed chunk is stale, so
        // the model receives the whole current file (real, up-to-date code)
        // rather than the outdated indexed slice.
        fs::write(&file, "the content has changed entirely").expect("rewrite corpus");
        let server = MyceliaServer::new(database, None, None);
        let output = server
            .retrieve(Parameters(RetrieveRequest {
                chunk_id: chunk_id.clone(),
            }))
            .expect("retrieve");

        assert!(output.contains("\"status\":\"file\""));
        assert!(output.contains("the content has changed entirely"));
        assert!(!output.contains("the original sourced content"));
    }

    #[test]
    fn find_self_heals_drift_and_returns_precise_headers() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("notes.txt");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(&file, "needle original content").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let indexed_hash = mycelia_core::find(&database, "needle", 5).expect("find")[0]
            .chunk
            .source_hash
            .clone();

        // Drift the source without re-indexing, then search. find must validate,
        // self-heal, and return a header reflecting the current file.
        fs::write(&file, "needle renewed content\nwith another line").expect("rewrite corpus");
        let server = MyceliaServer::new(database.clone(), None, None);
        let output = server
            .find(Parameters(FindRequest {
                query: "needle".to_owned(),
                limit: 5,
            }))
            .expect("find");

        let headers: serde_json::Value = serde_json::from_str(&output).expect("parse headers");
        let headers = headers.as_array().expect("header array");
        assert_eq!(headers.len(), 1);
        assert_ne!(
            headers[0]["source_hash"].as_str().expect("source hash"),
            indexed_hash,
            "header must carry the current source hash, not the stale one"
        );
        // The index converged on disk: the stale term is gone, the current term
        // is present.
        assert!(
            mycelia_core::find(&database, "original", 5)
                .expect("find stale")
                .is_empty()
        );
        assert_eq!(
            mycelia_core::find(&database, "renewed", 5)
                .expect("find fresh")
                .len(),
            1
        );
    }

    #[test]
    fn retrieve_self_heals_the_bound_index_on_drift() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let file = root.join("notes.txt");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(&file, "the original sourced content").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let chunk_id = mycelia_core::find(&database, "original", 5).expect("find")[0]
            .chunk
            .id
            .clone();

        fs::write(&file, "the renewed sourced content").expect("rewrite corpus");
        let server = MyceliaServer::new(database.clone(), None, None);
        server
            .retrieve(Parameters(RetrieveRequest { chunk_id }))
            .expect("retrieve");

        // The server quietly re-indexed the drifted file: the stale term is gone
        // and the current term is now a fresh, retrievable chunk.
        assert!(
            mycelia_core::find(&database, "original", 5)
                .expect("find stale")
                .is_empty()
        );
        let renewed = mycelia_core::find(&database, "renewed", 5).expect("find fresh");
        assert_eq!(renewed.len(), 1);
        let outcome = mycelia_core::retrieve(&database, renewed[0].chunk.id.as_str())
            .expect("retrieve")
            .expect("present");
        assert!(matches!(outcome, Retrieved::Ok { .. }));
    }

    #[test]
    fn serve_rejects_missing_database_before_starting_transport() {
        let temp = tempdir().expect("tempdir");
        let database = temp.path().join("missing.sqlite3");

        let error = serve(database.clone(), None, None, true).expect_err("missing database");

        assert_eq!(
            error,
            format!(
                "MCP database does not exist or is not a file: {}",
                database.display()
            )
        );
    }
}
