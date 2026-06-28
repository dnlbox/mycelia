use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use mycelia_core::{Direction, RelatedHit, RetrievalStrategy, Retrieved, SearchHeader};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
};
use serde::{Deserialize, Serialize};

use crate::log::CorpusLogger;
use crate::profile::{self, CorpusProfile};
use crate::project;
use crate::semantic::{FastEmbedProvider, MODEL_ID};

type McpToolResult<T> = std::result::Result<T, String>;

/// Shared lazy embedding provider. Tool handlers take `&self`, and
/// `embed_query` needs `&mut`, so the provider lives behind a `Mutex`; `Arc`
/// keeps `MyceliaServer` cloneable for the tool router.
type SharedProvider = Arc<Mutex<Option<FastEmbedProvider>>>;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindRequest {
    #[schemars(description = "Text to search for in the current project's Mycelia corpus")]
    query: String,
    #[serde(default = "default_limit")]
    #[schemars(description = "Maximum number of sourced headers to return")]
    limit: usize,
    #[serde(default)]
    #[schemars(
        description = "Optional corpus name. Use only when the user explicitly names another project."
    )]
    corpus: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RetrieveRequest {
    #[schemars(
        description = "Namespaced chunk identifier returned by Mycelia, for example corpus:hash"
    )]
    chunk_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct FindRelatedRequest {
    #[schemars(description = "The symbol (function, struct, type, ...) to map relationships for")]
    symbol: String,
    #[serde(default)]
    #[schemars(
        description = "Relationship direction: 'callers' (chunks that call the symbol) or 'callees' (definitions the symbol calls). Defaults to callers."
    )]
    direction: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Optional corpus name. Use only when the user explicitly names another project."
    )]
    corpus: Option<String>,
}

const SERVER_INSTRUCTIONS: &str = "Mycelia is the token-efficient orientation path for the current project corpus. For questions like where something is implemented, what supports a feature or language, which files touch an area, or which symbol defines behavior, call find first. Pass corpus only when the user explicitly names another project. Use retrieve only for the selected chunk ids that look relevant. Shell grep/read remain useful follow-up tools for exact line edits after Mycelia has narrowed the search.";

#[derive(Clone)]
struct MyceliaServer {
    target: ServerTarget,
    /// `Some` routes queries through embeddings; `None` serves lexical-only.
    provider: Option<SharedProvider>,
    tool_router: ToolRouter<Self>,
}

#[derive(Clone)]
enum ServerTarget {
    Registry {
        launch_cwd: PathBuf,
        fallback_corpus: Option<String>,
    },
    Database {
        database: PathBuf,
    },
}

struct ResolvedCorpus {
    name: Option<String>,
    root: Option<PathBuf>,
    database: PathBuf,
    log_path: Option<PathBuf>,
}

enum ResolveFailure {
    Message(String),
    NeedsCorpus {
        message: String,
        available: Vec<CorpusProfile>,
    },
}

#[derive(Serialize)]
struct NeedsCorpusResponse {
    status: &'static str,
    message: String,
    available_corpora: Vec<CorpusListing>,
}

#[derive(Serialize)]
struct CorpusListing {
    name: String,
    root: String,
    default: bool,
}

impl MyceliaServer {
    fn new(target: ServerTarget, provider: Option<SharedProvider>) -> Self {
        Self {
            target,
            provider,
            tool_router: Self::tool_router(),
        }
    }

    fn resolve_corpus(
        &self,
        corpus: Option<&str>,
    ) -> std::result::Result<ResolvedCorpus, ResolveFailure> {
        match &self.target {
            ServerTarget::Database { database } => {
                if corpus.is_some() {
                    return Err(ResolveFailure::Message(
                        "`corpus` is unavailable when serving an explicit --database".to_owned(),
                    ));
                }
                Ok(ResolvedCorpus {
                    name: None,
                    root: None,
                    database: database.clone(),
                    log_path: None,
                })
            }
            ServerTarget::Registry {
                launch_cwd,
                fallback_corpus,
            } => {
                let available = profile::list().map_err(ResolveFailure::Message)?;
                if let Some(name) = corpus {
                    return match available.into_iter().find(|profile| profile.name == name) {
                        Some(profile) => Ok(resolved_profile(profile)),
                        None => Err(ResolveFailure::NeedsCorpus {
                            message: format!("unknown corpus {name:?}; choose an available corpus"),
                            available: profile::list().map_err(ResolveFailure::Message)?,
                        }),
                    };
                }

                // Project-local config takes precedence over the registry.
                if let Some(result) = project::resolve_from_cwd(launch_cwd) {
                    return result
                        .map(|r| ResolvedCorpus {
                            name: Some(r.name),
                            root: Some(r.root),
                            database: r.database,
                            log_path: Some(r.log_path),
                        })
                        .map_err(ResolveFailure::Message);
                }

                if let Ok(profile) = profile::infer_from_cwd(launch_cwd) {
                    return Ok(resolved_profile(profile));
                }

                if let Some(name) = fallback_corpus {
                    return profile::get(name)
                        .map(resolved_profile)
                        .map_err(ResolveFailure::Message);
                }

                match available.len() {
                    0 => Err(ResolveFailure::NeedsCorpus {
                        message:
                            "no corpora registered; run `mycelia init` or `mycelia setup` first"
                                .to_owned(),
                        available,
                    }),
                    1 => Ok(resolved_profile(
                        available
                            .into_iter()
                            .next()
                            .expect("single available corpus"),
                    )),
                    _ => Err(ResolveFailure::NeedsCorpus {
                        message:
                            "current directory is not under a registered corpus; pass `corpus`"
                                .to_owned(),
                        available,
                    }),
                }
            }
        }
    }

    /// Runs retrieval for one resolved corpus and projects to headers. Routed
    /// mode loads the embedding model lazily and offline-only; if the corpus has
    /// no embeddings or the model is unavailable, the query falls back to
    /// reranked FTS5.
    fn find_headers(
        &self,
        resolved: &ResolvedCorpus,
        query: &str,
        limit: usize,
    ) -> McpToolResult<Vec<SearchHeader>> {
        match &self.provider {
            Some(provider) => {
                if !mycelia_core::has_embeddings(&resolved.database, MODEL_ID)
                    .map_err(|error| error.to_string())?
                {
                    return mycelia_core::find_headers(&resolved.database, query, limit)
                        .map_err(|error| error.to_string());
                }

                let mut guard = provider.lock().map_err(|error| error.to_string())?;
                if guard.is_none() {
                    match FastEmbedProvider::load(&resolved.database) {
                        Ok(loaded) => *guard = Some(loaded),
                        Err(error) => {
                            eprintln!(
                                "mycelia: embedding model unavailable, serving lexical retrieval: {error}"
                            );
                            return mycelia_core::find_headers(&resolved.database, query, limit)
                                .map_err(|error| error.to_string());
                        }
                    }
                }
                let provider = guard
                    .as_mut()
                    .ok_or_else(|| "embedding provider unavailable".to_owned())?;
                mycelia_core::find_headers_with_embeddings(
                    &resolved.database,
                    query,
                    limit,
                    RetrievalStrategy::Routed,
                    provider,
                )
            }
            None => mycelia_core::find_headers(&resolved.database, query, limit),
        }
        .map_err(|error| error.to_string())
    }

    fn search_json(&self, request: &FindRequest) -> McpToolResult<String> {
        let resolved = match self.resolve_corpus(request.corpus.as_deref()) {
            Ok(resolved) => resolved,
            Err(error) => return resolve_failure_json(self, error),
        };
        let headers = self.find_headers(&resolved, &request.query, request.limit)?;

        // Validate the sources behind the returned headers against disk. If any
        // drifted, self-heal them and re-rank once so the headers stay precise.
        let paths: Vec<String> = headers
            .iter()
            .map(|header| header.source_path.clone())
            .collect();
        let drifted = mycelia_core::drifted_sources(&resolved.database, &paths)
            .map_err(|error| error.to_string())?;
        let headers = if drifted.is_empty() {
            headers
        } else {
            for path in &drifted {
                let _ = mycelia_core::refresh_source(&resolved.database, path);
            }
            self.find_headers(&resolved, &request.query, request.limit)?
        };

        if let Some(logger) = self.logger_for(&resolved) {
            logger.log_find(&request.query, &headers);
        }

        let value = headers_json(&headers, resolved.name.as_deref());
        serde_json::to_string(&value).map_err(|error| error.to_string())
    }

    fn logger_for(&self, resolved: &ResolvedCorpus) -> Option<CorpusLogger> {
        resolved
            .log_path
            .clone()
            .map(|path| CorpusLogger::open(path, resolved.root.clone()))
    }

    fn instructions(&self) -> String {
        // Collect corpus names: project-local first (if any), then registry,
        // skipping duplicates so a corpus registered in both is listed once.
        let mut names: Vec<String> = Vec::new();
        if let ServerTarget::Registry { launch_cwd, .. } = &self.target
            && let Some(Ok(r)) = project::resolve_from_cwd(launch_cwd)
        {
            names.push(r.name);
        }
        for p in profile::list().unwrap_or_default() {
            if !names.contains(&p.name) {
                names.push(p.name);
            }
        }
        if names.is_empty() {
            return SERVER_INSTRUCTIONS.to_owned();
        }
        format!(
            "{SERVER_INSTRUCTIONS} Available corpora: {}.",
            names.join(", ")
        )
    }
}

#[tool_router]
impl MyceliaServer {
    #[tool(
        description = "Cheap first-pass codebase orientation over the current project's Mycelia corpus by default. Pass corpus only to search a different project the user has named. Use before grep/read when locating implementations, supported features, related files, symbols, or concepts. Returns ranked source headers with paths, line ranges, signatures or synopses, scores, and namespaced chunk ids without opening full files."
    )]
    fn find(&self, Parameters(request): Parameters<FindRequest>) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Alias for find with a more explicit name: search the current project's indexed codebase before grep/read to cheaply locate relevant files, symbols, features, or implementation areas. Pass corpus only when the user names another project."
    )]
    fn search_codebase(
        &self,
        Parameters(request): Parameters<FindRequest>,
    ) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Alias for find tuned for implementation hunts. Use for questions like where is X implemented, what supports Y, or which source chunks define a feature before opening raw files. Pass corpus only when the user names another project."
    )]
    fn locate_implementation(
        &self,
        Parameters(request): Parameters<FindRequest>,
    ) -> McpToolResult<String> {
        self.search_json(&request)
    }

    #[tool(
        description = "Fetch the exact body for one namespaced chunk id selected from a prior Mycelia search result. Use after find/search_codebase/locate_implementation has identified the specific source chunk worth reading."
    )]
    fn retrieve(&self, Parameters(request): Parameters<RetrieveRequest>) -> McpToolResult<String> {
        let (corpus_name, raw_chunk_id) = split_namespaced_chunk_id(&request.chunk_id);
        let resolved = match self.resolve_corpus(corpus_name) {
            Ok(resolved) => resolved,
            Err(error) => return resolve_failure_json(self, error),
        };

        let outcome = mycelia_core::retrieve(&resolved.database, raw_chunk_id)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("chunk not found: {}", request.chunk_id))?;

        if let Some(logger) = self.logger_for(&resolved) {
            let source_path = match &outcome {
                mycelia_core::Retrieved::Ok { chunk } => chunk.source_path.as_str(),
                mycelia_core::Retrieved::File { source_path, .. } => source_path.as_str(),
                mycelia_core::Retrieved::Unavailable { source_path, .. } => source_path.as_str(),
            };
            logger.log_retrieve(&request.chunk_id, source_path);
        }

        let mut value = serde_json::to_value(&outcome).map_err(|error| error.to_string())?;

        if let Some(source_path) = drifted_source(&outcome)
            && mycelia_core::refresh_source(&resolved.database, source_path).is_err()
            && let Some(object) = value.as_object_mut()
        {
            object.insert(
                "refresh_hint".to_owned(),
                serde_json::Value::String(
                    "index self-update failed; run `mycelia refresh` when convenient".to_owned(),
                ),
            );
        }

        if let Some(name) = resolved.name.as_deref()
            && let Some(object) = value.as_object_mut()
        {
            object.insert(
                "corpus".to_owned(),
                serde_json::Value::String(name.to_owned()),
            );
        }

        serde_json::to_string(&value).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Map deterministic `calls` relationships for a code symbol: its callers (chunks that call it) or its callees (definitions it calls). Use for structural questions grep cannot answer cheaply, such as who calls X or what X depends on. Each result is a sourced definition header with a namespaced chunk id and the call site; an ambiguous name is returned with every candidate and resolved=false rather than guessing. Pass corpus only when the user names another project."
    )]
    fn find_related(
        &self,
        Parameters(request): Parameters<FindRelatedRequest>,
    ) -> McpToolResult<String> {
        let direction = parse_direction(request.direction.as_deref())?;
        let resolved = match self.resolve_corpus(request.corpus.as_deref()) {
            Ok(resolved) => resolved,
            Err(error) => return resolve_failure_json(self, error),
        };
        let hits = mycelia_core::find_relationships(&resolved.database, &request.symbol, direction)
            .map_err(|error| error.to_string())?;
        let value = related_json(&hits, direction, &request.symbol, resolved.name.as_deref());
        serde_json::to_string(&value).map_err(|error| error.to_string())
    }

    #[tool(
        description = "List registered Mycelia corpora. Use only when a request names another project ambiguously or Mycelia returns needs_corpus."
    )]
    fn list_corpora(&self) -> McpToolResult<String> {
        let all = profile::list().map_err(|error| error.to_string())?;
        let default_name = match &self.target {
            ServerTarget::Registry { launch_cwd, .. } => {
                if let Some(Ok(r)) = project::resolve_from_cwd(launch_cwd) {
                    Some(r.name)
                } else {
                    profile::infer_from_cwd(launch_cwd).ok().map(|p| p.name)
                }
            }
            ServerTarget::Database { .. } => None,
        };
        serde_json::to_string(&corpus_listings(all, default_name.as_deref()))
            .map_err(|error| error.to_string())
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
            .with_instructions(self.instructions())
    }
}

pub(crate) fn serve(
    database: Option<PathBuf>,
    fallback_corpus: Option<String>,
    launch_cwd: PathBuf,
    lexical: bool,
) -> McpToolResult<()> {
    let target = if let Some(database) = database {
        if !database.is_file() {
            return Err(format!(
                "MCP database does not exist or is not a file: {}",
                database.display()
            ));
        }
        ServerTarget::Database { database }
    } else {
        if let Some(name) = &fallback_corpus {
            profile::get(name)?;
        }
        ServerTarget::Registry {
            launch_cwd,
            fallback_corpus,
        }
    };

    // Routed mode loads the embedding model lazily on the first query for any
    // corpus that already has embeddings. `load` is offline-only, so serving,
    // find, and connect never trigger hidden model downloads.
    let provider = (!lexical).then(|| Arc::new(Mutex::new(None)));

    let embeddings_status = if provider.is_some() {
        "routed"
    } else {
        "lexical"
    };
    if let ServerTarget::Registry { launch_cwd, .. } = &target {
        // Log serve-start to the project-local log if a .mycelia/config.toml is
        // present; fall through to registry corpora regardless.
        if let Some(Ok(r)) = project::resolve_from_cwd(launch_cwd) {
            if let Some(parent) = r.log_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            CorpusLogger::open(r.log_path, Some(r.root))
                .log_serve_start(MODEL_ID, embeddings_status);
        }
        if let Ok(corpora) = profile::list() {
            for corpus in corpora {
                if let Ok(path) = profile::log_path_for(&corpus.name) {
                    CorpusLogger::open(path, Some(corpus.root))
                        .log_serve_start(MODEL_ID, embeddings_status);
                }
            }
        }
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;

    runtime.block_on(async move {
        let service = MyceliaServer::new(target, provider)
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

fn resolved_profile(profile: CorpusProfile) -> ResolvedCorpus {
    let log_path = profile::log_path_for(&profile.name).ok();
    ResolvedCorpus {
        name: Some(profile.name),
        root: Some(profile.root),
        database: profile.database,
        log_path,
    }
}

fn split_namespaced_chunk_id(chunk_id: &str) -> (Option<&str>, &str) {
    chunk_id
        .split_once(':')
        .map_or((None, chunk_id), |(corpus, id)| (Some(corpus), id))
}

fn headers_json(headers: &[SearchHeader], corpus: Option<&str>) -> serde_json::Value {
    let mut value = serde_json::to_value(headers).unwrap_or_else(|_| serde_json::json!([]));
    if let Some(corpus) = corpus
        && let Some(items) = value.as_array_mut()
    {
        for item in items {
            if let Some(object) = item.as_object_mut() {
                if let Some(raw_id) = object.get("chunk_id").and_then(|value| value.as_str()) {
                    object.insert(
                        "chunk_id".to_owned(),
                        serde_json::Value::String(format!("{corpus}:{raw_id}")),
                    );
                }
                object.insert(
                    "corpus".to_owned(),
                    serde_json::Value::String(corpus.to_owned()),
                );
            }
        }
    }
    value
}

fn parse_direction(value: Option<&str>) -> McpToolResult<Direction> {
    match value {
        None => Ok(Direction::Callers),
        Some(raw) => match raw.to_ascii_lowercase().as_str() {
            "callers" => Ok(Direction::Callers),
            "callees" => Ok(Direction::Callees),
            other => Err(format!(
                "unknown direction {other:?}; use 'callers' or 'callees'"
            )),
        },
    }
}

/// Serializes relationship hits, namespacing each definition's `chunk_id` with
/// the corpus so the model can retrieve it, and wrapping them with the query
/// symbol and direction so the response is self-describing.
fn related_json(
    hits: &[RelatedHit],
    direction: Direction,
    symbol: &str,
    corpus: Option<&str>,
) -> serde_json::Value {
    let mut items = serde_json::to_value(hits).unwrap_or_else(|_| serde_json::json!([]));
    if let Some(corpus) = corpus
        && let Some(array) = items.as_array_mut()
    {
        for item in array {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            let Some(definition) = object
                .get_mut("definition")
                .and_then(|value| value.as_object_mut())
            else {
                continue;
            };
            let Some(raw_id) = definition
                .get("chunk_id")
                .and_then(|value| value.as_str())
                .map(str::to_owned)
            else {
                continue;
            };
            definition.insert(
                "chunk_id".to_owned(),
                serde_json::Value::String(format!("{corpus}:{raw_id}")),
            );
        }
    }
    serde_json::json!({
        "symbol": symbol,
        "direction": direction.as_str(),
        "corpus": corpus,
        "relationships": items,
    })
}

fn resolve_failure_json(server: &MyceliaServer, error: ResolveFailure) -> McpToolResult<String> {
    match error {
        ResolveFailure::Message(message) => Err(message),
        ResolveFailure::NeedsCorpus { message, available } => {
            let default_name = match &server.target {
                ServerTarget::Registry { launch_cwd, .. } => {
                    if let Some(Ok(r)) = project::resolve_from_cwd(launch_cwd) {
                        Some(r.name)
                    } else {
                        profile::infer_from_cwd(launch_cwd).ok().map(|p| p.name)
                    }
                }
                ServerTarget::Database { .. } => None,
            };
            serde_json::to_string(&NeedsCorpusResponse {
                status: "needs_corpus",
                message,
                available_corpora: corpus_listings(available, default_name.as_deref()),
            })
            .map_err(|error| error.to_string())
        }
    }
}

fn corpus_listings(all: Vec<CorpusProfile>, default_name: Option<&str>) -> Vec<CorpusListing> {
    all.into_iter()
        .map(|profile| CorpusListing {
            default: default_name == Some(profile.name.as_str()),
            name: profile.name,
            root: profile.root.display().to_string(),
        })
        .collect()
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
        let server = MyceliaServer::new(ServerTarget::Database { database }, None);

        let output = server
            .find(Parameters(FindRequest {
                query: "precise".to_owned(),
                limit: 5,
                corpus: None,
            }))
            .expect("find");

        assert!(output.contains("\"source_path\":\"notes.txt\""));
        assert!(output.contains("\"chunk_id\""));
        assert!(!output.contains("\"text\""));
    }

    #[test]
    fn find_related_returns_sourced_callers() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(
            root.join("util.rs"),
            "pub fn helper() -> i32 {\n    42\n}\n",
        )
        .expect("write util");
        fs::write(root.join("app.rs"), "fn run() -> i32 {\n    helper()\n}\n").expect("write app");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let server = MyceliaServer::new(ServerTarget::Database { database }, None);

        let output = server
            .find_related(Parameters(FindRelatedRequest {
                symbol: "helper".to_owned(),
                direction: Some("callers".to_owned()),
                corpus: None,
            }))
            .expect("find_related");

        assert!(output.contains("\"direction\":\"callers\""), "{output}");
        assert!(output.contains("\"symbol\":\"run\""), "{output}");
        assert!(output.contains("\"resolved\":true"), "{output}");

        let error = server
            .find_related(Parameters(FindRelatedRequest {
                symbol: "helper".to_owned(),
                direction: Some("sideways".to_owned()),
                corpus: None,
            }))
            .expect_err("invalid direction");
        assert!(error.contains("unknown direction"), "{error}");
    }

    #[test]
    fn retrieve_reports_missing_chunk() {
        let temp = tempdir().expect("tempdir");
        let root = temp.path().join("corpus");
        let database = temp.path().join("mycelia.sqlite3");
        fs::create_dir_all(&root).expect("create corpus");
        fs::write(root.join("notes.txt"), "indexed content").expect("write corpus");
        mycelia_core::index_corpus(&root, &database).expect("index corpus");
        let server = MyceliaServer::new(ServerTarget::Database { database }, None);

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

        fs::write(&file, "the content has changed entirely").expect("rewrite corpus");
        let server = MyceliaServer::new(ServerTarget::Database { database }, None);
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

        fs::write(&file, "needle renewed content\nwith another line").expect("rewrite corpus");
        let server = MyceliaServer::new(
            ServerTarget::Database {
                database: database.clone(),
            },
            None,
        );
        let output = server
            .find(Parameters(FindRequest {
                query: "needle".to_owned(),
                limit: 5,
                corpus: None,
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
        let server = MyceliaServer::new(
            ServerTarget::Database {
                database: database.clone(),
            },
            None,
        );
        server
            .retrieve(Parameters(RetrieveRequest { chunk_id }))
            .expect("retrieve");

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

        let error = serve(
            Some(database.clone()),
            None,
            temp.path().to_path_buf(),
            true,
        )
        .expect_err("missing database");

        assert_eq!(
            error,
            format!(
                "MCP database does not exist or is not a file: {}",
                database.display()
            )
        );
    }
}
