mod log;
mod mcp;
mod profile;
mod project;
mod semantic;

use clap::{Args, Parser, Subcommand, ValueEnum};
use mycelia_core::{
    self, ChunkRecord, Direction, EmbeddingReport, EvaluationCase, EvaluationReport, IndexReport,
    PairedEvaluationReport, RelatedHit, RetrievalStrategy, Retrieved, SearchHeader,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;

type Result<T> = std::result::Result<T, String>;

const MISSING_EMBEDDINGS_HINT: &str = "corpus has no embeddings for this model; run `mycelia embed` first or pass `--strategy fts5-reranked`";
const CONNECT_HARNESS_HELP: &str = "Supported harnesses:\n  codex            Codex CLI (~/.codex/config.toml)\n  claude-code      Claude Code CLI (`claude mcp add`)\n  claude-desktop   Claude Desktop app config\n  cursor           Cursor MCP config\n  antigravity      Antigravity / Gemini CLI (~/.gemini/antigravity/mcp_config.json)\n  opencode         OpenCode CLI (~/.config/opencode/opencode.json)\n  kilo             Kilo CLI (~/.config/kilo/kilo.json)";

enum Retrieval {
    Embedded(Box<semantic::FastEmbedProvider>),
    Lexical(RetrievalStrategy),
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum Strategy {
    Substring,
    Fts5,
    Fts5Reranked,
    Vector,
    Hybrid,
    #[default]
    Routed,
}

impl From<Strategy> for RetrievalStrategy {
    fn from(strategy: Strategy) -> Self {
        match strategy {
            Strategy::Substring => Self::Substring,
            Strategy::Fts5 => Self::Fts5,
            Strategy::Fts5Reranked => Self::Fts5Reranked,
            Strategy::Vector => Self::Vector,
            Strategy::Hybrid => Self::Hybrid,
            Strategy::Routed => Self::Routed,
        }
    }
}

impl Strategy {
    fn uses_embeddings(self) -> bool {
        matches!(self, Self::Vector | Self::Hybrid | Self::Routed)
    }
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum GraphDirection {
    /// Chunks that call the symbol.
    #[default]
    Callers,
    /// Definitions of the symbols the symbol calls.
    Callees,
}

impl From<GraphDirection> for Direction {
    fn from(direction: GraphDirection) -> Self {
        match direction {
            GraphDirection::Callers => Self::Callers,
            GraphDirection::Callees => Self::Callees,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Harness {
    Codex,
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
    Antigravity,
    #[value(name = "opencode")]
    OpenCode,
    Kilo,
}

impl Harness {
    fn server_label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::Antigravity => "Antigravity",
            Self::OpenCode => "OpenCode",
            Self::Kilo => "Kilo",
        }
    }
}

#[derive(Parser, Debug)]
#[command(name = "mycelia")]
#[command(version)]
#[command(about = "Local, content-aware knowledge index for AI agents")]
#[command(
    long_about = "Mycelia indexes a local corpus, exposes token-efficient find/retrieve tools, and wires the index into local AI harnesses."
)]
#[command(arg_required_else_help = true)]
#[command(
    after_help = "Typical flow:\n  mycelia setup\n  mycelia connect codex\n  mycelia status\n\nUseful checks:\n  mycelia list\n  mycelia stats\n\nThe old `mycelia corpus ...` commands are retired. Use `setup`, `status`, and `list`."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Initialise Mycelia in the current project. Creates `.mycelia/`, indexes,
    /// and optionally wires guidance into existing root AGENTS.md or CLAUDE.md.
    Init {
        /// Project root. Defaults to the git root of the current directory.
        path: Option<PathBuf>,
        /// Skip embedding (useful offline or in tests).
        #[arg(long)]
        no_embed: bool,
    },
    /// Register, index, and embed a corpus. Defaults to the git root of the
    /// current directory. Idempotent: re-running refreshes the corpus.
    Setup {
        /// Corpus root directory. Defaults to the git root of cwd.
        path: Option<PathBuf>,
        /// Corpus name. Defaults to the basename of the root directory.
        #[arg(long)]
        name: Option<String>,
        /// Skip the embedding step (useful offline or in tests).
        #[arg(long)]
        no_embed: bool,
    },
    /// Wire the corpus into a supported AI harness.
    #[command(after_help = CONNECT_HARNESS_HELP)]
    Connect {
        /// Target harness to configure. Run `mycelia connect --help` to list supported values.
        #[arg(value_enum, value_name = "HARNESS")]
        harness: Option<Harness>,
        #[command(flatten)]
        target: CwdTarget,
    },
    /// Show token-savings statistics aggregated from the activity log.
    Stats {
        /// Also print the last N find/retrieve log events.
        #[arg(long, default_value_t = 0)]
        recent: usize,
        #[command(flatten)]
        target: CwdTarget,
    },
    /// Show index health: chunk count, embedding coverage, last refresh.
    Status {
        #[command(flatten)]
        target: CwdTarget,
    },
    /// Force a full re-index and re-embed. A manual fallback; query-time
    /// freshness guarantees correctness without requiring this.
    Refresh {
        #[command(flatten)]
        target: CwdTarget,
    },
    /// List all registered corpora. Marks the one matching cwd with *.
    List,
    /// Remove a corpus profile, database, and activity log after confirmation.
    Delete {
        #[command(flatten)]
        target: CwdTarget,
    },
    /// Retired command group. Use setup, status, and list instead.
    Corpus,
    /// CI-oriented commands for preparing deterministic per-commit indexes.
    Ci {
        #[command(subcommand)]
        command: CiCommand,
    },
    /// Search the index and print ranked headers.
    Find {
        query: String,
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long, default_value_t = 10)]
        limit: usize,
        #[arg(long, value_enum, default_value_t)]
        strategy: Strategy,
        #[arg(long)]
        json: bool,
    },
    /// Retrieve one chunk by its identifier.
    Retrieve {
        chunk_id: Option<String>,
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long)]
        json: bool,
    },
    /// Show `calls` relationships for a symbol: its callers or its callees.
    Graph {
        symbol: String,
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long, value_enum, default_value_t)]
        direction: GraphDirection,
        #[arg(long)]
        json: bool,
    },
    /// Run a manifest-driven retrieval evaluation (power use).
    Eval {
        manifest: PathBuf,
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long, value_enum, default_value_t)]
        strategy: Strategy,
        /// Emit a paired Mycelia vs grep/read baseline report.
        #[arg(long)]
        paired: bool,
        #[arg(long)]
        json: bool,
    },
    /// Index a corpus into a database (diagnostic / explicit path mode).
    Index {
        #[command(flatten)]
        target: IndexTarget,
        #[arg(long)]
        json: bool,
    },
    /// Compute and store embeddings for a database (diagnostic / explicit).
    Embed {
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long)]
        json: bool,
    },
    /// Start the stdio MCP server (launched by the harness, not by hand).
    Serve {
        #[command(flatten)]
        target: AnyTarget,
        /// Resolve the project-local corpus from this root instead of the launch
        /// directory. `connect` writes this for project-local corpora so the
        /// server hits the same index and log no matter where the harness
        /// spawns it.
        #[arg(long)]
        project_root: Option<PathBuf>,
        /// Serve lexical-only (reranked FTS5), skipping the embedding model load.
        #[arg(long)]
        lexical: bool,
    },
}

#[derive(Subcommand, Debug)]
enum CiCommand {
    /// Build or refresh the project-local index for the current git commit.
    Prepare {
        /// Project root. Defaults to the git root of the current directory.
        path: Option<PathBuf>,
        /// Keep the CI path lexical-only. This is the default.
        #[arg(long)]
        no_embed: bool,
        /// Alias for --no-embed. This is the default.
        #[arg(long)]
        lexical: bool,
        /// Also refresh embeddings. This may download the embedding model.
        #[arg(long, conflicts_with_all = ["no_embed", "lexical"])]
        embed: bool,
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Export the project-local index as a CI artifact directory.
    Export {
        /// Artifact directory to create or update.
        artifact_dir: PathBuf,
        /// Project root. Defaults to the git root of the current directory.
        path: Option<PathBuf>,
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Verify a CI artifact manifest against the current checkout.
    Verify {
        /// Artifact directory containing manifest.json and db files.
        artifact_dir: PathBuf,
        /// Project root. Defaults to the git root of the current directory.
        path: Option<PathBuf>,
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
    /// Verify and import a CI artifact into the project-local index.
    Import {
        /// Artifact directory containing manifest.json and db files.
        artifact_dir: PathBuf,
        /// Project root. Defaults to the git root of the current directory.
        path: Option<PathBuf>,
        /// Emit a machine-readable report.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Serialize)]
struct CiPrepareReport {
    project_root: PathBuf,
    database: PathBuf,
    cache_key: String,
    git_commit: String,
    schema_version: i64,
    extractor_versions: Vec<String>,
    project_config_hash: String,
    extractor_hash: String,
    lexical: bool,
    chunks_written: usize,
    files_indexed: usize,
    files_removed: usize,
    files_rejected: usize,
    github_env_written: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ArtifactManifest {
    mycelia_version: String,
    schema_version: i64,
    project_name: String,
    git_commit: String,
    source_root_hash: String,
    extractors: Vec<String>,
    embedding_model: Option<String>,
    db_files: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CiArtifactReport {
    artifact_dir: PathBuf,
    manifest: ArtifactManifest,
    database: PathBuf,
    db_files: Vec<String>,
    status: String,
}

// Target resolution

/// Target for journey commands: infers corpus from cwd when neither flag is
/// given. `--database` is the diagnostic escape hatch.
#[derive(Args, Debug)]
struct CwdTarget {
    #[arg(long, conflicts_with = "database")]
    corpus: Option<String>,
    #[arg(long, conflicts_with = "corpus")]
    database: Option<PathBuf>,
}

/// Target for find/retrieve/eval/embed/serve: explicit flags only (no
/// cwd-inference). When neither flag is given, cwd-inference is attempted.
#[derive(Args, Debug)]
struct AnyTarget {
    #[arg(long, conflicts_with = "database")]
    corpus: Option<String>,
    #[arg(long, conflicts_with = "corpus")]
    database: Option<PathBuf>,
}

/// Target for `index`: requires root + database, or a corpus name.
#[derive(Args, Debug)]
struct IndexTarget {
    root: Option<PathBuf>,
    #[arg(long, conflicts_with = "corpus")]
    database: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["root", "database"])]
    corpus: Option<String>,
}

struct ResolvedCorpus {
    database: PathBuf,
    corpus_name: Option<String>,
    corpus_root: Option<PathBuf>,
    log_path: Option<PathBuf>,
    /// True when resolved from `.mycelia/config.toml` rather than the registry.
    project_local: bool,
}

struct ResolvedServe {
    database: Option<PathBuf>,
    fallback_corpus: Option<String>,
}

impl CwdTarget {
    fn resolve(self) -> Result<ResolvedCorpus> {
        match (self.corpus, self.database) {
            (Some(name), None) => {
                let p = profile::get(&name)?;
                let log_path = profile::log_path_for(&p.name).ok();
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                    log_path,
                    project_local: false,
                })
            }
            (None, Some(db)) => Ok(ResolvedCorpus {
                database: db,
                corpus_name: None,
                corpus_root: None,
                log_path: None,
                project_local: false,
            }),
            (None, None) => resolve_from_cwd(),
            (Some(_), Some(_)) => {
                Err("--corpus and --database cannot be used together".to_string())
            }
        }
    }
}

impl AnyTarget {
    fn resolve(self) -> Result<ResolvedCorpus> {
        match (self.corpus, self.database) {
            (Some(name), None) => {
                let p = profile::get(&name)?;
                let log_path = profile::log_path_for(&p.name).ok();
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                    log_path,
                    project_local: false,
                })
            }
            (None, Some(db)) => Ok(ResolvedCorpus {
                database: db,
                corpus_name: None,
                corpus_root: None,
                log_path: None,
                project_local: false,
            }),
            (None, None) => resolve_from_cwd(),
            (Some(_), Some(_)) => {
                Err("--corpus and --database cannot be used together".to_string())
            }
        }
    }

    fn resolve_database(self) -> Result<PathBuf> {
        Ok(self.resolve()?.database)
    }

    fn resolve_serve(self) -> Result<ResolvedServe> {
        match (self.corpus, self.database) {
            (Some(name), None) => {
                let profile = profile::get(&name)?;
                Ok(ResolvedServe {
                    database: None,
                    fallback_corpus: Some(profile.name),
                })
            }
            (None, Some(db)) => Ok(ResolvedServe {
                database: Some(db),
                fallback_corpus: None,
            }),
            (None, None) => Ok(ResolvedServe {
                database: None,
                fallback_corpus: None,
            }),
            (Some(_), Some(_)) => {
                Err("--corpus and --database cannot be used together".to_string())
            }
        }
    }
}

impl IndexTarget {
    fn resolve(self) -> Result<(PathBuf, PathBuf)> {
        match (self.root, self.database, self.corpus) {
            (Some(root), Some(database), None) => Ok((root, database)),
            (None, None, Some(name)) => {
                let profile = profile::get(&name)?;
                Ok((profile.root, profile.database))
            }
            (None, None, None) => Err(
                "use either <root> --database <path> or --corpus <name>".to_string(),
            ),
            _ => Err(
                "explicit indexing requires both <root> and --database; named indexing uses only --corpus"
                    .to_string(),
            ),
        }
    }
}

/// Resolution ladder for cwd-inferred targets (no explicit `--corpus` or
/// `--database`): `.mycelia/config.toml` walk-up first, then the legacy
/// registry, then a clear error naming `mycelia init`.
fn resolve_from_cwd() -> Result<ResolvedCorpus> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot determine current directory: {error}"))?;

    if let Some(result) = project::resolve_from_cwd(&cwd) {
        let r = result?;
        return Ok(ResolvedCorpus {
            database: r.database,
            corpus_name: Some(r.name),
            corpus_root: Some(r.root),
            log_path: Some(r.log_path),
            project_local: true,
        });
    }

    if let Ok(p) = profile::infer_from_cwd(&cwd) {
        let log_path = profile::log_path_for(&p.name).ok();
        return Ok(ResolvedCorpus {
            database: p.database,
            corpus_name: Some(p.name),
            corpus_root: Some(p.root),
            log_path,
            project_local: false,
        });
    }

    Err(
        "no Mycelia project found; run `mycelia init` to set up project-local indexing, \
         or `mycelia setup` to register a corpus"
            .to_string(),
    )
}

// Main dispatch

#[derive(Deserialize)]
struct EvaluationManifest {
    limit: usize,
    cases: Vec<EvaluationCase>,
}

fn validate_evaluation_manifest(
    manifest: &EvaluationManifest,
    manifest_path: &Path,
    resolved: &ResolvedCorpus,
) -> Result<()> {
    let corpus_root = match &resolved.corpus_root {
        Some(root) => Some(root.clone()),
        None => mycelia_core::corpus_root(&resolved.database).map_err(|e| e.to_string())?,
    };

    if let Some(root) = corpus_root {
        let canonical_root = root
            .canonicalize()
            .map_err(|e| format!("invalid corpus root {}: {e}", root.display()))?;
        let canonical_manifest = manifest_path.canonicalize().map_err(|e| {
            format!(
                "invalid evaluation manifest {}: {e}",
                manifest_path.display()
            )
        })?;
        if canonical_manifest.starts_with(&canonical_root) {
            return Err(format!(
                "evaluation manifest must live outside the indexed corpus ({} is under {})",
                canonical_manifest.display(),
                canonical_root.display()
            ));
        }
    }

    for case in &manifest.cases {
        for path in &case.required_files {
            if !is_safe_relative_source_path(path) {
                return Err(format!(
                    "evaluation case '{}' has an invalid required_files entry: {path}",
                    case.name
                ));
            }
        }
    }

    Ok(())
}

fn is_safe_relative_source_path(path: &str) -> bool {
    let path = Path::new(path);
    if path.as_os_str().is_empty() {
        return false;
    }
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

pub fn run() -> Result<()> {
    run_from(std::env::args_os())
}

pub fn run_from<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            error.exit();
        }
        Err(error) => return Err(normalize_clap_error(error)),
    };

    match cli.command {
        Command::Init { path, no_embed } => cmd_init(path, no_embed),
        Command::Setup {
            path,
            name,
            no_embed,
        } => cmd_setup(path, name, no_embed),
        Command::Connect { harness, target } => cmd_connect(harness, target),
        Command::Stats { recent, target } => cmd_stats(target, recent),
        Command::Status { target } => cmd_status(target),
        Command::Refresh { target } => cmd_refresh(target),
        Command::List => cmd_list(),
        Command::Delete { target } => cmd_delete(target),
        Command::Corpus => Err(
            "`mycelia corpus` has been retired. Use `mycelia setup`, `mycelia status`, or `mycelia list`."
                .to_string(),
        ),
        Command::Ci { command } => match command {
            CiCommand::Prepare {
                path,
                no_embed,
                lexical,
                embed,
                json,
            } => cmd_ci_prepare(path, !embed || no_embed || lexical, json),
            CiCommand::Export {
                artifact_dir,
                path,
                json,
            } => cmd_ci_export(artifact_dir, path, json),
            CiCommand::Verify {
                artifact_dir,
                path,
                json,
            } => cmd_ci_verify(artifact_dir, path, json),
            CiCommand::Import {
                artifact_dir,
                path,
                json,
            } => cmd_ci_import(artifact_dir, path, json),
        },

        Command::Find {
            query,
            target,
            limit,
            strategy,
            json,
        } => {
            let resolved = target.resolve()?;
            let headers = find_headers(&resolved.database, &query, limit, strategy)?;
            emit_output(if json {
                serde_json::to_string(&headers).map_err(|e| e.to_string())?
            } else {
                format_search_headers(&headers)
            });
            Ok(())
        }

        Command::Retrieve {
            chunk_id,
            target,
            json,
        } => {
            let database = target.resolve_database()?;
            let chunk_id = chunk_id.ok_or_else(|| "retrieve requires a chunk id".to_string())?;
            let outcome = mycelia_core::retrieve(&database, &chunk_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("chunk not found: {chunk_id}"))?;
            emit_output(if json {
                serde_json::to_string(&outcome).map_err(|e| e.to_string())?
            } else {
                format_retrieved(&outcome)
            });
            Ok(())
        }

        Command::Graph {
            symbol,
            target,
            direction,
            json,
        } => {
            let resolved = target.resolve()?;
            let hits = mycelia_core::find_relationships(&resolved.database, &symbol, direction.into())
                .map_err(|e| e.to_string())?;
            emit_output(if json {
                serde_json::to_string(&hits).map_err(|e| e.to_string())?
            } else {
                format_related_hits(&symbol, direction, &hits)
            });
            Ok(())
        }

        Command::Eval {
            manifest,
            target,
            strategy,
            paired,
            json,
        } => {
            let resolved = target.resolve()?;
            let manifest_path = manifest;
            let contents = fs::read_to_string(&manifest_path)
                .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;
            let manifest: EvaluationManifest = serde_json::from_str(&contents)
                .map_err(|e| format!("invalid evaluation manifest: {e}"))?;
            validate_evaluation_manifest(&manifest, &manifest_path, &resolved)?;
            if paired {
                let report = eval_paired_with_strategy(
                    &resolved.database,
                    &manifest.cases,
                    manifest.limit,
                    strategy,
                )?;
                emit_output(if json {
                    serde_json::to_string(&report).map_err(|e| e.to_string())?
                } else {
                    format_paired_evaluation_report(&report)
                });
                return Ok(());
            }

            let report =
                eval_with_strategy(&resolved.database, &manifest.cases, manifest.limit, strategy)?;
            emit_output(if json {
                serde_json::to_string(&report).map_err(|e| e.to_string())?
            } else {
                format_evaluation_report(&report)
            });
            Ok(())
        }

        Command::Index { target, json } => {
            let (root, database) = target.resolve()?;
            let report = mycelia_core::index_corpus(&root, &database).map_err(|e| e.to_string())?;
            emit_output(if json {
                serde_json::to_string(&report).map_err(|e| e.to_string())?
            } else {
                format_index_report(&report)
            });
            Ok(())
        }

        Command::Embed { target, json } => {
            let database = target.resolve_database()?;
            let mut provider = prepare_embedding_provider(&database)?;
            let report = mycelia_core::refresh_embeddings(&database, &mut provider)
                .map_err(|e| e.to_string())?;
            emit_output(if json {
                serde_json::to_string(&report).map_err(|e| e.to_string())?
            } else {
                format_embedding_report(&report)
            });
            Ok(())
        }

        Command::Serve {
            target,
            project_root,
            lexical,
        } => {
            let resolved = target.resolve_serve()?;
            // A pinned --project-root makes resolution independent of the cwd
            // the harness happens to spawn us in; otherwise fall back to it.
            let launch_cwd = match project_root {
                Some(root) => root,
                None => std::env::current_dir()
                    .map_err(|error| format!("cannot determine current directory: {error}"))?,
            };
            mcp::serve(
                resolved.database,
                resolved.fallback_corpus,
                launch_cwd,
                lexical,
            )
        }
    }
}

fn cmd_ci_prepare(path: Option<PathBuf>, lexical: bool, json: bool) -> Result<()> {
    let root = resolve_git_root(path)?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "cannot derive project name from path".to_string())?;
    let commit = git_output(&root, ["rev-parse", "HEAD"])?;

    project::init_project(&root, &name)?;
    let database = root.join(".mycelia").join("db").join("index.sqlite3");
    let index_report = mycelia_core::index_corpus(&root, &database).map_err(|e| e.to_string())?;

    if !lexical {
        let mut provider = prepare_embedding_provider(&database)?;
        let _ = mycelia_core::refresh_embeddings(&database, &mut provider)
            .map_err(|e| e.to_string())?;
    }

    let _ = mycelia_core::corpus_status(&database).map_err(|e| e.to_string())?;

    let schema_version = mycelia_core::schema_version();
    let extractor_versions = mycelia_core::extractor_versions()
        .iter()
        .map(|version| (*version).to_string())
        .collect::<Vec<_>>();
    let extractor_hash = hash_text(extractor_versions.join("\n").as_str());
    let project_config = root.join(".mycelia").join("config.toml");
    let project_config_hash = hash_file(&project_config)?;
    let cache_key = format!(
        "mycelia-{}-schema{schema_version}-extractors{extractor_hash}-config{project_config_hash}-commit{commit}",
        env!("CARGO_PKG_VERSION")
    );

    let github_env_written = write_github_env(&root, &database, &cache_key, &commit)?;
    let report = CiPrepareReport {
        project_root: root,
        database,
        cache_key,
        git_commit: commit,
        schema_version,
        extractor_versions,
        project_config_hash,
        extractor_hash,
        lexical,
        chunks_written: index_report.chunks_written,
        files_indexed: index_report.indexed,
        files_removed: index_report.removed,
        files_rejected: index_report.rejected,
        github_env_written,
    };

    emit_output(if json {
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    } else {
        format_ci_prepare_report(&report)
    });
    Ok(())
}

fn cmd_ci_export(artifact_dir: PathBuf, path: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_git_root(path)?;
    let database = root.join(".mycelia").join("db").join("index.sqlite3");
    let manifest = build_artifact_manifest(&root, &database)?;
    let artifact_dir = prepare_artifact_dir(&artifact_dir)?;
    let artifact_db_dir = artifact_dir.join("db");
    fs::create_dir_all(&artifact_db_dir)
        .map_err(|e| format!("failed to create {}: {e}", artifact_db_dir.display()))?;

    for db_file in &manifest.db_files {
        let source = root.join(".mycelia").join(db_file);
        let destination = artifact_dir.join(db_file);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        fs::copy(&source, &destination).map_err(|e| {
            format!(
                "failed to copy {} to {}: {e}",
                source.display(),
                destination.display()
            )
        })?;
    }

    let manifest_path = artifact_dir.join("manifest.json");
    let manifest_json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(&manifest_path, manifest_json)
        .map_err(|e| format!("failed to write {}: {e}", manifest_path.display()))?;

    emit_artifact_report(artifact_dir, database, manifest, "exported", json)
}

fn cmd_ci_verify(artifact_dir: PathBuf, path: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_git_root(path)?;
    let artifact_dir = artifact_dir
        .canonicalize()
        .map_err(|e| format!("invalid artifact dir {}: {e}", artifact_dir.display()))?;
    let manifest = verify_artifact_for_root(&artifact_dir, &root)?;
    let database = root.join(".mycelia").join("db").join("index.sqlite3");
    emit_artifact_report(artifact_dir, database, manifest, "verified", json)
}

fn cmd_ci_import(artifact_dir: PathBuf, path: Option<PathBuf>, json: bool) -> Result<()> {
    let root = resolve_git_root(path)?;
    let artifact_dir = artifact_dir
        .canonicalize()
        .map_err(|e| format!("invalid artifact dir {}: {e}", artifact_dir.display()))?;
    let manifest = verify_artifact_for_root(&artifact_dir, &root)?;
    let project_name = project_name_for_root(&root)?;
    project::init_project(&root, &project_name)?;

    let db_dir = root.join(".mycelia").join("db");
    fs::create_dir_all(&db_dir)
        .map_err(|e| format!("failed to create {}: {e}", db_dir.display()))?;
    for db_file in &manifest.db_files {
        let source = artifact_dir.join(db_file);
        let destination = root.join(".mycelia").join(db_file);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }
        fs::copy(&source, &destination).map_err(|e| {
            format!(
                "failed to copy {} to {}: {e}",
                source.display(),
                destination.display()
            )
        })?;
    }

    let database = root.join(".mycelia").join("db").join("index.sqlite3");
    let _ = mycelia_core::corpus_status(&database).map_err(|e| e.to_string())?;
    emit_artifact_report(artifact_dir, database, manifest, "imported", json)
}

fn build_artifact_manifest(root: &Path, database: &Path) -> Result<ArtifactManifest> {
    if !database.is_file() {
        return Err(format!(
            "project-local database not found: {}; run `mycelia ci prepare` first",
            database.display()
        ));
    }
    let status = mycelia_core::corpus_status(database).map_err(|e| e.to_string())?;
    Ok(ArtifactManifest {
        mycelia_version: env!("CARGO_PKG_VERSION").to_string(),
        schema_version: mycelia_core::schema_version(),
        project_name: project_name_for_root(root)?,
        git_commit: git_output(root, ["rev-parse", "HEAD"])?,
        source_root_hash: mycelia_core::source_root_hash(root).map_err(|e| e.to_string())?,
        extractors: extractor_versions(),
        embedding_model: status.embedding_model,
        db_files: db_files_for_database(database)?,
    })
}

fn verify_artifact_for_root(artifact_dir: &Path, root: &Path) -> Result<ArtifactManifest> {
    let manifest_path = artifact_dir.join("manifest.json");
    let contents = fs::read_to_string(&manifest_path)
        .map_err(|e| format!("failed to read {}: {e}", manifest_path.display()))?;
    let manifest: ArtifactManifest =
        serde_json::from_str(&contents).map_err(|e| format!("invalid artifact manifest: {e}"))?;

    verify_field(
        "mycelia_version",
        env!("CARGO_PKG_VERSION"),
        manifest.mycelia_version.as_str(),
    )?;
    verify_i64_field(
        "schema_version",
        mycelia_core::schema_version(),
        manifest.schema_version,
    )?;
    verify_field(
        "project_name",
        project_name_for_root(root)?.as_str(),
        manifest.project_name.as_str(),
    )?;
    verify_field(
        "git_commit",
        git_output(root, ["rev-parse", "HEAD"])?.as_str(),
        manifest.git_commit.as_str(),
    )?;
    verify_field(
        "source_root_hash",
        mycelia_core::source_root_hash(root)
            .map_err(|e| e.to_string())?
            .as_str(),
        manifest.source_root_hash.as_str(),
    )?;
    verify_vec_field(
        "extractors",
        extractor_versions().as_slice(),
        manifest.extractors.as_slice(),
    )?;
    verify_db_files(artifact_dir, manifest.db_files.as_slice())?;

    let artifact_database = artifact_dir.join("db").join("index.sqlite3");
    let status = mycelia_core::corpus_status(&artifact_database).map_err(|e| {
        format!(
            "artifact mismatch: db_files database could not be opened ({}): {e}",
            artifact_database.display()
        )
    })?;
    let artifact_root = mycelia_core::corpus_root(&artifact_database)
        .map_err(|e| format!("artifact mismatch: corpus_root could not be read: {e}"))?
        .ok_or_else(|| "artifact mismatch: corpus_root missing".to_string())?;
    let current_root = root
        .canonicalize()
        .map_err(|e| format!("invalid project root {}: {e}", root.display()))?;
    verify_field(
        "corpus_root",
        current_root.to_string_lossy().as_ref(),
        artifact_root.to_string_lossy().as_ref(),
    )?;
    verify_option_field(
        "embedding_model",
        manifest.embedding_model.as_deref(),
        status.embedding_model.as_deref(),
    )?;

    Ok(manifest)
}

fn prepare_artifact_dir(path: &Path) -> Result<PathBuf> {
    if path.exists() && !path.is_dir() {
        return Err(format!(
            "artifact path is not a directory: {}",
            path.display()
        ));
    }
    fs::create_dir_all(path).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    path.canonicalize()
        .map_err(|e| format!("invalid artifact dir {}: {e}", path.display()))
}

fn emit_artifact_report(
    artifact_dir: PathBuf,
    database: PathBuf,
    manifest: ArtifactManifest,
    status: &str,
    json: bool,
) -> Result<()> {
    let db_files = manifest.db_files.clone();
    let report = CiArtifactReport {
        artifact_dir,
        manifest,
        database,
        db_files,
        status: status.to_string(),
    };
    emit_output(if json {
        serde_json::to_string(&report).map_err(|e| e.to_string())?
    } else {
        format_ci_artifact_report(&report)
    });
    Ok(())
}

fn format_ci_artifact_report(report: &CiArtifactReport) -> String {
    format!(
        "status: {status}\n\
         artifact_dir: {artifact_dir}\n\
         database: {database}\n\
         git_commit: {git_commit}\n\
         source_root_hash: {source_root_hash}\n\
         db_files: {db_files}",
        status = report.status,
        artifact_dir = report.artifact_dir.display(),
        database = report.database.display(),
        git_commit = report.manifest.git_commit,
        source_root_hash = report.manifest.source_root_hash,
        db_files = report.db_files.join(",")
    )
}

fn project_name_for_root(root: &Path) -> Result<String> {
    if let Some(result) = project::resolve_from_cwd(root) {
        return Ok(result?.name);
    }
    root.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "cannot derive project name from path".to_string())
}

fn extractor_versions() -> Vec<String> {
    mycelia_core::extractor_versions()
        .iter()
        .map(|version| (*version).to_string())
        .collect()
}

fn db_files_for_database(database: &Path) -> Result<Vec<String>> {
    let db_dir = database
        .parent()
        .ok_or_else(|| format!("database path has no parent: {}", database.display()))?;
    let file_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("database path is not valid UTF-8: {}", database.display()))?;
    let mut files = Vec::new();
    for name in [
        file_name.to_string(),
        format!("{file_name}-wal"),
        format!("{file_name}-shm"),
    ] {
        let path = db_dir.join(&name);
        if path.exists() {
            files.push(format!("db/{name}"));
        }
    }
    if files.iter().all(|file| file != "db/index.sqlite3") {
        return Err("artifact mismatch: db_files missing db/index.sqlite3".to_string());
    }
    Ok(files)
}

fn verify_db_files(artifact_dir: &Path, db_files: &[String]) -> Result<()> {
    if db_files.is_empty() {
        return Err("artifact mismatch: db_files is empty".to_string());
    }
    if db_files.iter().all(|file| file != "db/index.sqlite3") {
        return Err("artifact mismatch: db_files missing db/index.sqlite3".to_string());
    }
    for file in db_files {
        let path = Path::new(file);
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(format!("artifact mismatch: db_files unsafe entry {file}"));
        }
        let full_path = artifact_dir.join(path);
        if !full_path.is_file() {
            return Err(format!("artifact mismatch: db_files missing file {file}"));
        }
    }
    Ok(())
}

fn verify_field(field: &str, expected: &str, found: &str) -> Result<()> {
    if expected == found {
        Ok(())
    } else {
        Err(format!(
            "artifact mismatch: {field} expected {expected}, found {found}"
        ))
    }
}

fn verify_i64_field(field: &str, expected: i64, found: i64) -> Result<()> {
    if expected == found {
        Ok(())
    } else {
        Err(format!(
            "artifact mismatch: {field} expected {expected}, found {found}"
        ))
    }
}

fn verify_vec_field(field: &str, expected: &[String], found: &[String]) -> Result<()> {
    if expected == found {
        Ok(())
    } else {
        Err(format!(
            "artifact mismatch: {field} expected {}, found {}",
            expected.join(","),
            found.join(",")
        ))
    }
}

fn verify_option_field(field: &str, expected: Option<&str>, found: Option<&str>) -> Result<()> {
    if expected == found {
        Ok(())
    } else {
        Err(format!(
            "artifact mismatch: {field} expected {}, found {}",
            expected.unwrap_or("<none>"),
            found.unwrap_or("<none>")
        ))
    }
}

fn resolve_git_root(path: Option<PathBuf>) -> Result<PathBuf> {
    let root = match path {
        Some(path) => path
            .canonicalize()
            .map_err(|e| format!("invalid path {}: {e}", path.display()))?,
        None => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("cannot determine current directory: {e}"))?;
            profile::git_root(&cwd)
                .ok_or_else(|| "not in a git repository; provide an explicit path".to_string())?
        }
    };

    if !root.is_dir() {
        return Err(format!(
            "project root is not a directory: {}",
            root.display()
        ));
    }
    Ok(root)
}

fn git_output<const N: usize>(root: &Path, args: [&str; N]) -> Result<String> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "failed to resolve git commit for {}: {}",
            root.display(),
            stderr.trim()
        ));
    }
    Ok(String::from_utf8(output.stdout)
        .map_err(|e| format!("git output was not UTF-8: {e}"))?
        .trim()
        .to_string())
}

fn hash_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    Ok(blake3::hash(bytes.as_slice()).to_hex().to_string())
}

fn hash_text(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex().to_string()
}

fn write_github_env(
    root: &Path,
    database: &Path,
    cache_key: &str,
    commit: &str,
) -> Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os("GITHUB_ENV").map(PathBuf::from) else {
        return Ok(None);
    };
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open GITHUB_ENV {}: {e}", path.display()))?;

    use std::io::Write;
    writeln!(file, "MYCELIA_PROJECT_ROOT={}", root.display())
        .map_err(|e| format!("failed to write GITHUB_ENV: {e}"))?;
    writeln!(file, "MYCELIA_DATABASE={}", database.display())
        .map_err(|e| format!("failed to write GITHUB_ENV: {e}"))?;
    writeln!(file, "MYCELIA_CACHE_KEY={cache_key}")
        .map_err(|e| format!("failed to write GITHUB_ENV: {e}"))?;
    writeln!(file, "MYCELIA_GIT_COMMIT={commit}")
        .map_err(|e| format!("failed to write GITHUB_ENV: {e}"))?;
    writeln!(
        file,
        "MYCELIA_SCHEMA_VERSION={}",
        mycelia_core::schema_version()
    )
    .map_err(|e| format!("failed to write GITHUB_ENV: {e}"))?;

    Ok(Some(path))
}

fn format_ci_prepare_report(report: &CiPrepareReport) -> String {
    let mode = if report.lexical {
        "lexical-only"
    } else {
        "embedded"
    };
    format!(
        "project_root: {root}\n\
         database: {database}\n\
         git_commit: {commit}\n\
         cache_key: {cache_key}\n\
         schema_version: {schema_version}\n\
         extractors: {extractors}\n\
         mode: {mode}\n\
         indexed: {chunks} chunks from {files} files ({removed} removed, {rejected} rejected)\n\
         env: {env_status}",
        root = report.project_root.display(),
        database = report.database.display(),
        commit = report.git_commit,
        cache_key = report.cache_key,
        schema_version = report.schema_version,
        extractors = report.extractor_versions.join(","),
        chunks = report.chunks_written,
        files = report.files_indexed,
        removed = report.files_removed,
        rejected = report.files_rejected,
        env_status = report
            .github_env_written
            .as_ref()
            .map(|path| format!("wrote {}", path.display()))
            .unwrap_or_else(|| "GITHUB_ENV not set".to_string())
    )
}

fn prepare_embedding_provider(database: &Path) -> Result<semantic::FastEmbedProvider> {
    eprintln!("Preparing embedding model...");
    eprintln!("  model: {}", semantic::MODEL_ID);
    eprintln!("  loading ONNX Runtime and model weights; first run may download model files");
    let provider = semantic::FastEmbedProvider::prepare(database).map_err(|e| e.to_string())?;
    eprintln!("  model ready");
    Ok(provider)
}

// Journey commands

fn cmd_init(path: Option<PathBuf>, no_embed: bool) -> Result<()> {
    let root = match path {
        Some(p) => p
            .canonicalize()
            .map_err(|e| format!("invalid path {}: {e}", p.display()))?,
        None => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("cannot determine current directory: {e}"))?;
            profile::git_root(&cwd)
                .ok_or_else(|| "not in a git repository; provide an explicit path".to_string())?
        }
    };

    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_owned)
        .ok_or_else(|| "cannot derive project name from path".to_string())?;

    eprintln!(
        "Initialising Mycelia project '{name}' at {}...",
        root.display()
    );

    project::init_project(&root, &name)?;
    eprintln!("  created .mycelia/");

    let database = root.join(".mycelia").join("db").join("index.sqlite3");

    eprintln!("Indexing...");
    let index_report = mycelia_core::index_corpus(&root, &database).map_err(|e| e.to_string())?;
    eprintln!(
        "  {} chunks from {} files ({} removed, {} rejected)",
        index_report.chunks_written,
        index_report.indexed,
        index_report.removed,
        index_report.rejected
    );

    if !no_embed {
        let mut provider = prepare_embedding_provider(&database)?;
        let report = mycelia_core::refresh_embeddings(&database, &mut provider)
            .map_err(|e| e.to_string())?;
        eprintln!(
            "  {} embedded, {} unchanged, {} stored",
            report.embedded,
            report.unchanged,
            format_bytes(report.storage_bytes as u64)
        );
    }

    // Consent-gated guidance: detect root instruction files, preview, and apply
    // only when the user confirms.
    let guidance_files = project::detect_guidance_files(&root);
    for file in &guidance_files {
        let rel = file.strip_prefix(&root).unwrap_or(file.as_path());
        eprintln!();
        eprintln!("Found {}", rel.display());
        eprintln!("Would add to {}:", rel.display());
        eprintln!("---");
        eprint!("{}", project::guidance_include_preview(file));
        eprintln!("---");
        eprint!("Add guidance include? [y/N] ");

        use std::io::Write;
        std::io::stderr().flush().ok();

        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("failed to read input: {e}"))?;

        if input.trim().eq_ignore_ascii_case("y") {
            project::insert_guidance_include(file)?;
            eprintln!("  added guidance to {}", rel.display());
        } else {
            eprintln!("  skipped");
        }
    }

    eprintln!();
    eprintln!("Done. Run `mycelia connect <harness>` to wire it into your AI tool.");
    Ok(())
}

fn cmd_setup(path: Option<PathBuf>, name_flag: Option<String>, no_embed: bool) -> Result<()> {
    // Determine root: explicit path or git root of cwd.
    let root = match path {
        Some(p) => p
            .canonicalize()
            .map_err(|e| format!("invalid path {}: {e}", p.display()))?,
        None => {
            let cwd = std::env::current_dir()
                .map_err(|e| format!("cannot determine current directory: {e}"))?;
            profile::git_root(&cwd)
                .ok_or_else(|| "not in a git repository; provide an explicit path".to_string())?
        }
    };

    // Determine name: explicit or basename of root.
    let name = match name_flag {
        Some(n) => n,
        None => root
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_owned)
            .ok_or_else(|| "cannot derive corpus name from path; use --name".to_string())?,
    };

    // Check for name collision (another corpus with the same name but a different root).
    if let Ok(existing) = profile::get(&name) {
        let existing_canonical = existing
            .root
            .canonicalize()
            .unwrap_or(existing.root.clone());
        if existing_canonical != root {
            return Err(format!(
                "corpus name '{name}' is already registered for a different root ({}); use --name to choose a different name",
                existing.root.display()
            ));
        }
    }

    eprintln!("Registering corpus '{name}' at {}...", root.display());
    let profile = profile::set(&name, &root)?;

    eprintln!("Indexing...");
    let index_report =
        mycelia_core::index_corpus(&profile.root, &profile.database).map_err(|e| e.to_string())?;
    eprintln!(
        "  {} chunks from {} files ({} removed, {} rejected)",
        index_report.chunks_written,
        index_report.indexed,
        index_report.removed,
        index_report.rejected
    );

    if !no_embed {
        let mut provider = prepare_embedding_provider(&profile.database)?;
        let report = mycelia_core::refresh_embeddings(&profile.database, &mut provider)
            .map_err(|e| e.to_string())?;
        eprintln!(
            "  {} embedded, {} unchanged, {} stored",
            report.embedded,
            report.unchanged,
            format_bytes(report.storage_bytes as u64)
        );
    }

    eprintln!("Done. Run `mycelia connect <harness>` to wire it into your AI tool.");
    Ok(())
}

fn cmd_connect(harness: Option<Harness>, target: CwdTarget) -> Result<()> {
    let harness = harness.ok_or_else(|| {
        format!("connect requires a harness.\n\n{CONNECT_HARNESS_HELP}\n\nUsage: mycelia connect <harness>")
    })?;
    let resolved = target.resolve()?;
    if resolved.corpus_name.is_none() {
        return Err(
            "connect requires a named corpus; use --corpus or run from a registered directory"
                .to_string(),
        );
    }

    let binary =
        std::env::current_exe().map_err(|e| format!("failed to determine binary path: {e}"))?;
    let binary_str = binary
        .to_str()
        .ok_or_else(|| "binary path contains non-UTF-8 characters".to_string())?
        .to_owned();

    let server_name = "mycelia";
    // Pin the target so the server resolves the same index and log regardless of
    // the cwd the harness spawns it in: a project root for project-local corpora,
    // the corpus name for registry corpora.
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("");
    let project_root = resolved
        .corpus_root
        .as_deref()
        .and_then(Path::to_str)
        .map(str::to_owned);
    let serve_args: Vec<&str> = match (resolved.project_local, project_root.as_deref()) {
        (true, Some(root)) => vec!["serve", "--project-root", root],
        (true, None) => vec!["serve"],
        (false, _) => vec!["serve", "--corpus", corpus_name],
    };

    match harness {
        Harness::ClaudeCode => connect_claude_code(server_name, &binary_str, &serve_args),
        Harness::ClaudeDesktop => connect_json_file(
            server_name,
            &binary_str,
            &serve_args,
            claude_desktop_config_path()?,
            harness.server_label(),
        ),
        Harness::Cursor => connect_json_file(
            server_name,
            &binary_str,
            &serve_args,
            cursor_config_path()?,
            harness.server_label(),
        ),
        Harness::Codex => connect_codex(server_name, &binary_str, &serve_args),
        Harness::Antigravity => connect_json_file(
            server_name,
            &binary_str,
            &serve_args,
            antigravity_config_path()?,
            harness.server_label(),
        ),
        Harness::OpenCode => connect_json_file(
            server_name,
            &binary_str,
            &serve_args,
            opencode_config_path()?,
            harness.server_label(),
        ),
        Harness::Kilo => connect_json_file(
            server_name,
            &binary_str,
            &serve_args,
            kilo_config_path()?,
            harness.server_label(),
        ),
    }
}

fn connect_claude_code(server_name: &str, binary: &str, args: &[&str]) -> Result<()> {
    // `claude mcp add --scope user <name> -- <binary> [args...]`
    // The `--` separates mcp-add flags from the server command, since the server
    // args include `--corpus` which would otherwise be parsed by mcp add.
    let status = std::process::Command::new("claude")
        .arg("mcp")
        .arg("add")
        .arg("--scope")
        .arg("user")
        .arg(server_name)
        .arg("--")
        .arg(binary)
        .args(args)
        .status()
        .map_err(|e| {
            format!("failed to run `claude`: {e}; is the Claude Code CLI installed and in PATH?")
        })?;

    if status.success() {
        eprintln!("Wired '{server_name}' into Claude Code. Restart Claude Code to activate.");
        Ok(())
    } else {
        Err(format!(
            "`claude mcp add` exited with status {status}; the entry may already exist. Run `claude mcp remove {server_name}` first if you want to reset it"
        ))
    }
}

fn connect_json_file(
    server_name: &str,
    binary: &str,
    args: &[&str],
    config_path: PathBuf,
    harness_label: &str,
) -> Result<()> {
    // Read existing config or start from scratch.
    let mut root: serde_json::Value = if config_path.is_file() {
        let text = fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;
        let stripped = project::strip_json_comments(&text);
        serde_json::from_str(&stripped)
            .map_err(|e| format!("invalid JSON in {}: {e}", config_path.display()))?
    } else {
        serde_json::json!({})
    };

    let entry = serde_json::json!({
        "command": binary,
        "args": args,
    });

    // Merge into mcpServers object.
    root.as_object_mut()
        .ok_or_else(|| format!("unexpected root type in {}", config_path.display()))?
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("mcpServers is not an object in {}", config_path.display()))?
        .retain(|name, _| name == server_name || !name.starts_with("mycelia-"));
    root.as_object_mut()
        .ok_or_else(|| format!("unexpected root type in {}", config_path.display()))?
        .get_mut("mcpServers")
        .and_then(|value| value.as_object_mut())
        .ok_or_else(|| format!("mcpServers is not an object in {}", config_path.display()))?
        .insert(server_name.to_owned(), entry);

    // Write back.
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(&config_path, json)
        .map_err(|e| format!("failed to write {}: {e}", config_path.display()))?;

    eprintln!(
        "Wired '{server_name}' into {harness_label} ({}).",
        config_path.display()
    );
    eprintln!("Restart {harness_label} to activate.");
    Ok(())
}

fn claude_desktop_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(home.join("Library/Application Support/Claude/claude_desktop_config.json"))
}

fn cursor_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(home.join(".cursor/mcp.json"))
}

fn codex_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(home.join(".codex/config.toml"))
}

fn antigravity_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    Ok(home.join(".gemini/antigravity/mcp_config.json"))
}

fn opencode_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    let jsonc = home.join(".config/opencode/opencode.jsonc");
    if jsonc.exists() {
        Ok(jsonc)
    } else {
        Ok(home.join(".config/opencode/opencode.json"))
    }
}

fn kilo_config_path() -> Result<PathBuf> {
    let home = home_dir()?;
    let jsonc = home.join(".config/kilo/kilo.jsonc");
    if jsonc.exists() {
        Ok(jsonc)
    } else {
        Ok(home.join(".config/kilo/kilo.json"))
    }
}

fn connect_codex(server_name: &str, binary: &str, args: &[&str]) -> Result<()> {
    let config_path = codex_config_path()?;
    let mut document = if config_path.is_file() {
        let text = fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;
        text.parse::<toml_edit::DocumentMut>()
            .map_err(|e| format!("invalid TOML in {}: {e}", config_path.display()))?
    } else {
        toml_edit::DocumentMut::new()
    };

    if !document.as_table().contains_key("mcp_servers") {
        document["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let servers = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| format!("mcp_servers is not a table in {}", config_path.display()))?;
    let legacy = servers
        .iter()
        .filter(|(name, _)| *name != server_name && name.starts_with("mycelia-"))
        .map(|(name, _)| name.to_owned())
        .collect::<Vec<_>>();
    for name in legacy {
        servers.remove(&name);
    }

    let mut entry = toml_edit::Table::new();
    entry["command"] = toml_edit::value(binary);
    let mut arg_array = toml_edit::Array::new();
    for arg in args {
        arg_array.push(*arg);
    }
    entry["args"] = toml_edit::value(arg_array);
    servers.insert(server_name, toml_edit::Item::Table(entry));

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    fs::write(&config_path, document.to_string())
        .map_err(|e| format!("failed to write {}: {e}", config_path.display()))?;

    eprintln!(
        "Wired '{server_name}' into Codex ({}).",
        config_path.display()
    );
    eprintln!("Restart Codex to activate.");
    Ok(())
}

fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME environment variable is not set".to_string())
}

fn cmd_stats(target: CwdTarget, recent: usize) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("<unnamed>");
    let log_path = resolved.log_path.clone();

    let stats = log_path.as_deref().map(log::read_stats).unwrap_or_default();

    println!("corpus:            {corpus_name}");
    println!("queries answered:  {}", stats.queries);

    if stats.queries == 0 {
        println!("(no find queries logged yet. Start a serve session to accumulate data)");
        print_recent_events(log_path.as_deref(), recent);
        return Ok(());
    }

    println!(
        "tokens via Mycelia:  ~{}   (avg {:.0} / answer)",
        stats.actual_tokens,
        stats.actual_tokens as f64 / stats.queries as f64
    );

    if stats.has_cold && stats.cold_tokens > 0 {
        let ratio = stats.cold_tokens as f64 / stats.actual_tokens.max(1) as f64;
        println!(
            "tokens if cold read: ~{}   (avg {:.0} / answer)",
            stats.cold_tokens,
            stats.cold_tokens as f64 / stats.queries as f64
        );
        println!(
            "estimated savings:   ~{ratio:.1}x  ({} tokens)",
            stats.cold_tokens.saturating_sub(stats.actual_tokens)
        );
    }
    print_recent_events(log_path.as_deref(), recent);
    Ok(())
}

fn print_recent_events(log_path: Option<&Path>, recent: usize) {
    if recent == 0 {
        return;
    }
    println!();
    println!("recent activity:");
    let events = log_path
        .map(|path| log::recent_events(path, recent))
        .unwrap_or_default();
    if events.is_empty() {
        println!("  (no find/retrieve events logged yet)");
    } else {
        for event in events {
            println!("  {event}");
        }
    }
}

fn cmd_status(target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("<unnamed>");

    let db_stats = mycelia_core::corpus_status(&resolved.database).map_err(|e| e.to_string())?;

    let log_path = resolved.log_path.clone();

    let last_serve = log_path
        .as_deref()
        .and_then(log::last_serve_start)
        .unwrap_or_else(|| "never".to_string());
    let last_refresh = database_modified_utc(&resolved.database)
        .unwrap_or_else(|| "unknown, run `mycelia refresh`".to_string());

    let embedding_line = match &db_stats.embedding_model {
        Some(model) => {
            let coverage = if db_stats.chunk_count > 0 {
                format!(
                    "{} / {} chunks",
                    db_stats.embedding_count, db_stats.chunk_count
                )
            } else {
                "0 / 0 chunks".to_string()
            };
            let status = if db_stats.embedding_count >= db_stats.chunk_count {
                "current"
            } else {
                "incomplete, run `mycelia refresh`"
            };
            format!("{coverage}  ({status})\nmodel:             {model}")
        }
        None => "none, run `mycelia refresh` to embed".to_string(),
    };

    let db_size = format_bytes(db_stats.db_size_bytes);

    let graph_line = if db_stats.chunk_count > 0 && db_stats.symbol_count == 0 {
        "none, run `mycelia refresh` to populate the call graph".to_string()
    } else {
        format!(
            "{} edges over {} symbols",
            db_stats.edge_count, db_stats.symbol_count
        )
    };

    println!("corpus:            {corpus_name}");
    println!("index:             {} chunks", db_stats.chunk_count);
    println!("embeddings:        {embedding_line}");
    println!("graph:             {graph_line}");
    println!("last serve:        {last_serve}");
    println!("last refresh:      {last_refresh}");
    println!("db size:           {db_size}");

    Ok(())
}

fn cmd_refresh(target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("<unnamed>");
    let root = resolved.corpus_root.ok_or_else(|| {
        "refresh requires a named corpus (cannot determine root from --database alone)".to_string()
    })?;

    eprintln!("Refreshing corpus '{corpus_name}'...");
    eprintln!("Indexing...");
    let report =
        mycelia_core::reindex_corpus(&root, &resolved.database).map_err(|e| e.to_string())?;
    eprintln!(
        "  {} chunks from {} files ({} removed, {} rejected)",
        report.chunks_written, report.indexed, report.removed, report.rejected
    );
    let mut provider = prepare_embedding_provider(&resolved.database)?;
    let embedding_report = mycelia_core::refresh_embeddings(&resolved.database, &mut provider)
        .map_err(|e| e.to_string())?;
    eprintln!(
        "  {} embedded, {} unchanged, {} stored",
        embedding_report.embedded,
        embedding_report.unchanged,
        format_bytes(embedding_report.storage_bytes as u64)
    );
    eprintln!("Done.");
    Ok(())
}

fn cmd_list() -> Result<()> {
    let all = profile::list()?;
    if all.is_empty() {
        println!("(no registered corpora. Run `mycelia setup` to register one)");
        return Ok(());
    }

    let cwd = std::env::current_dir().ok();
    let inferred_name = cwd.as_deref().and_then(|cwd| {
        if let Some(Ok(r)) = project::resolve_from_cwd(cwd) {
            return Some(r.name);
        }
        profile::infer_from_cwd(cwd).ok().map(|p| p.name)
    });

    println!("   {:<20} root", "name");
    for profile in &all {
        let marker = if inferred_name.as_deref() == Some(&profile.name) {
            "*"
        } else {
            " "
        };
        println!(
            "{marker:<2} {:<20} {}",
            profile.name,
            profile.root.display()
        );
    }
    Ok(())
}

fn cmd_delete(target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved
        .corpus_name
        .as_ref()
        .ok_or_else(|| "delete requires a named corpus; use --corpus".to_string())?;

    let db_path = &resolved.database;
    let log_path = profile::log_path_for(corpus_name).ok();
    let profile_path = profile::profile_path_for(corpus_name).ok();

    println!("Will delete:");
    if let Some(p) = &profile_path {
        println!("  profile:  {}", p.display());
    }
    println!("  database: {}", db_path.display());
    if let Some(p) = &log_path {
        println!("  log:      {}", p.display());
    }
    print!("\nType 'yes' to confirm: ");
    use std::io::Write;
    std::io::stdout().flush().ok();

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| format!("failed to read confirmation: {e}"))?;

    if input.trim() != "yes" {
        println!("Cancelled.");
        return Ok(());
    }

    // Remove profile.
    let _ = profile::remove(corpus_name);

    // Remove database and SQLite sidecars.
    for sidecar in &["", "-wal", "-shm"] {
        let path = if sidecar.is_empty() {
            db_path.clone()
        } else {
            PathBuf::from(format!("{}{sidecar}", db_path.display()))
        };
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }

    // Remove log.
    if let Some(p) = &log_path {
        let _ = fs::remove_file(p);
        // Also remove backup.
        let _ = fs::remove_file(p.with_extension("log.bak"));
    }

    println!("Deleted corpus '{corpus_name}'.");
    Ok(())
}

// Retrieval helpers

fn find_headers(
    database: &Path,
    query: &str,
    limit: usize,
    strategy: Strategy,
) -> Result<Vec<SearchHeader>> {
    if strategy.uses_embeddings() {
        match resolve_retrieval(database, strategy)? {
            Retrieval::Embedded(mut provider) => mycelia_core::find_headers_with_embeddings(
                database,
                query,
                limit,
                strategy.into(),
                &mut *provider,
            ),
            Retrieval::Lexical(lexical) => {
                mycelia_core::find_headers_with_strategy(database, query, limit, lexical)
            }
        }
    } else {
        mycelia_core::find_headers_with_strategy(database, query, limit, strategy.into())
    }
    .map_err(|e| e.to_string())
}

fn eval_with_strategy(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: Strategy,
) -> Result<EvaluationReport> {
    if strategy.uses_embeddings() {
        match resolve_retrieval(database, strategy)? {
            Retrieval::Embedded(mut provider) => mycelia_core::evaluate_with_embeddings(
                database,
                cases,
                limit,
                strategy.into(),
                &mut *provider,
            ),
            Retrieval::Lexical(lexical) => {
                mycelia_core::evaluate_with_strategy(database, cases, limit, lexical)
            }
        }
    } else {
        mycelia_core::evaluate_with_strategy(database, cases, limit, strategy.into())
    }
    .map_err(|e| e.to_string())
}

fn eval_paired_with_strategy(
    database: &Path,
    cases: &[EvaluationCase],
    limit: usize,
    strategy: Strategy,
) -> Result<PairedEvaluationReport> {
    let mycelia = eval_with_strategy(database, cases, limit, strategy)?;
    let baseline =
        mycelia_core::evaluate_baseline(database, cases, limit).map_err(|e| e.to_string())?;
    Ok(mycelia_core::pair_evaluation_reports(mycelia, baseline))
}

fn resolve_retrieval(database: &Path, strategy: Strategy) -> Result<Retrieval> {
    let available =
        mycelia_core::has_embeddings(database, semantic::MODEL_ID).map_err(|e| e.to_string())?;
    if !available {
        return match strategy {
            Strategy::Routed => Ok(Retrieval::Lexical(RetrievalStrategy::Fts5Reranked)),
            _ => Err(MISSING_EMBEDDINGS_HINT.to_owned()),
        };
    }

    match semantic::FastEmbedProvider::load(database) {
        Ok(provider) => Ok(Retrieval::Embedded(Box::new(provider))),
        Err(error) => match strategy {
            Strategy::Routed => {
                eprintln!("mycelia: embedding model unavailable, using reranked FTS5: {error}");
                Ok(Retrieval::Lexical(RetrievalStrategy::Fts5Reranked))
            }
            _ => Err(error.to_string()),
        },
    }
}

// Formatting

fn normalize_clap_error(error: clap::Error) -> String {
    let message = error.to_string();
    message
        .strip_prefix("error: ")
        .unwrap_or(&message)
        .trim_end()
        .to_string()
}

fn emit_output(output: String) {
    if !output.is_empty() {
        println!("{output}");
    }
}

fn format_bytes(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const KB: u64 = 1024;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn database_modified_utc(database: &Path) -> Option<String> {
    let modified = fs::metadata(database).ok()?.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(log::format_epoch_utc(secs))
}

fn format_index_report(report: &IndexReport) -> String {
    [
        format!("discovered: {}", report.discovered),
        format!("indexed: {}", report.indexed),
        format!("unchanged: {}", report.unchanged),
        format!("removed: {}", report.removed),
        format!("rejected: {}", report.rejected),
        format!("chunks_written: {}", report.chunks_written),
        format!("code_parse_fallbacks: {}", report.code_parse_fallbacks),
        format!("elapsed_ms: {}", report.elapsed_ms),
    ]
    .join("\n")
}

fn format_embedding_report(report: &EmbeddingReport) -> String {
    [
        format!("model_id: {}", report.model_id),
        format!("dimensions: {}", report.dimensions),
        format!("embedded: {}", report.embedded),
        format!("unchanged: {}", report.unchanged),
        format!("removed_other_models: {}", report.removed_other_models),
        format!("storage_bytes: {}", report.storage_bytes),
        format!("elapsed_ms: {}", report.elapsed_ms),
    ]
    .join("\n")
}

fn format_search_headers(headers: &[SearchHeader]) -> String {
    headers
        .iter()
        .map(format_search_header)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_search_header(header: &SearchHeader) -> String {
    let mut lines = vec![
        format!("id: {}", header.chunk_id),
        format!("path: {}", header.source_path),
        format!(
            "byte range: {}..{}",
            header.span.byte_start, header.span.byte_end
        ),
        format!(
            "line range: {}..{}",
            header.span.line_start, header.span.line_end
        ),
        format!("extractor: {}", header.extractor),
        format!("score: {:.6}", header.score),
    ];

    if let Some(signature) = &header.signature {
        lines.push(format!("signature: {signature}"));
    }

    lines.push(format!("synopsis: {}", header.synopsis));
    lines.join("\n")
}

fn format_related_hits(symbol: &str, direction: GraphDirection, hits: &[RelatedHit]) -> String {
    if hits.is_empty() {
        return match direction {
            GraphDirection::Callers => format!("no callers found for {symbol}"),
            GraphDirection::Callees => format!("no callees found for {symbol}"),
        };
    }
    hits.iter()
        .map(format_related_hit)
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn format_related_hit(hit: &RelatedHit) -> String {
    let mut lines = vec![
        format!("symbol: {}", hit.symbol),
        format!("id: {}", hit.definition.chunk_id),
        format!("path: {}", hit.definition.source_path),
        format!(
            "line range: {}..{}",
            hit.definition.span.line_start, hit.definition.span.line_end
        ),
        format!("call site line: {}", hit.call_site.line_start),
    ];
    if let Some(signature) = &hit.definition.signature {
        lines.push(format!("signature: {signature}"));
    }
    if !hit.resolved {
        lines.push(format!(
            "ambiguous: name resolves to {} definitions",
            hit.definition_count
        ));
    }
    lines.join("\n")
}

fn format_retrieved(outcome: &Retrieved) -> String {
    match outcome {
        Retrieved::Ok { chunk } => format_chunk_record(chunk, None),
        Retrieved::File {
            source_path,
            line_start,
            line_end,
            text,
        } => [
            "status: file".to_owned(),
            format!("path: {source_path}"),
            format!("line range: {line_start}..{line_end}"),
            format!("text:\n{text}"),
        ]
        .join("\n"),
        Retrieved::Unavailable {
            chunk_id,
            source_path,
            message,
        } => format!("status: unavailable\nid: {chunk_id}\npath: {source_path}\n{message}"),
    }
}

fn format_chunk_record(record: &ChunkRecord, score: Option<f64>) -> String {
    let mut lines = vec![
        format!("id: {}", record.id),
        format!("path: {}", record.source_path),
        format!(
            "byte range: {}..{}",
            record.span.byte_start, record.span.byte_end
        ),
        format!(
            "line range: {}..{}",
            record.span.line_start, record.span.line_end
        ),
        format!("extractor: {}", record.extractor),
    ];

    if let Some(score) = score {
        lines.push(format!("score: {score:.6}"));
    }

    lines.push(format!("text:\n{}", record.text));
    lines.join("\n")
}

fn format_evaluation_report(report: &EvaluationReport) -> String {
    let mut lines = vec![
        format!("strategy: {}", report.strategy),
        format!("limit: {}", report.limit),
        format!("cases: {}", report.cases),
        format!("hits: {}", report.hits),
        format!("hit_rate: {:.4}", report.hit_rate),
        format!("mean_reciprocal_rank: {:.4}", report.mean_reciprocal_rank),
        format!("elapsed_ms: {}", report.elapsed_ms),
        format!(
            "token_answered_queries: {}",
            report.token_usage.answered_queries
        ),
        format!(
            "token_find_headers: {}",
            report.token_usage.find_header_tokens
        ),
        format!(
            "token_retrieved_bodies: {}",
            report.token_usage.retrieved_body_tokens
        ),
        format!("token_answer_total: {}", report.token_usage.answer_tokens),
        format!(
            "tokens_per_answer: {:.2}",
            report.token_usage.tokens_per_answer
        ),
    ];
    if let Some(cold_source_tokens) = report.token_usage.cold_source_tokens {
        lines.push(format!("token_cold_sources: {cold_source_tokens}"));
    }
    if let Some(cold_tokens_per_answer) = report.token_usage.cold_tokens_per_answer {
        lines.push(format!(
            "cold_tokens_per_answer: {cold_tokens_per_answer:.2}"
        ));
    }
    lines.extend(report.results.iter().map(|result| {
        format!(
            "{}: {}",
            result.name,
            result
                .rank
                .map_or_else(|| "miss".to_owned(), |rank| format!("rank {rank}"))
        )
    }));
    lines.join("\n")
}

fn format_paired_evaluation_report(report: &PairedEvaluationReport) -> String {
    let mut lines = vec![
        "mycelia:".to_owned(),
        format!("  strategy: {}", report.mycelia.strategy),
        format!("  hit_rate: {:.4}", report.mycelia.hit_rate),
        format!(
            "  mean_reciprocal_rank: {:.4}",
            report.mycelia.mean_reciprocal_rank
        ),
        format!(
            "  tokens_per_answer: {:.2}",
            report.mycelia.token_usage.tokens_per_answer
        ),
        "baseline:".to_owned(),
        format!("  name: {}", report.baseline.name),
        format!("  hit_rate: {:.4}", report.baseline.hit_rate),
        format!(
            "  mean_reciprocal_rank: {:.4}",
            report.baseline.mean_reciprocal_rank
        ),
        format!(
            "  tokens_per_answer: {:.2}",
            report.baseline.token_usage.tokens_per_answer
        ),
        "comparison:".to_owned(),
        format!("  hit_rate_delta: {:.4}", report.comparison.hit_rate_delta),
        format!(
            "  mean_reciprocal_rank_delta: {:.4}",
            report.comparison.mean_reciprocal_rank_delta
        ),
        format!(
            "  tokens_per_answer_delta: {:.2}",
            report.comparison.tokens_per_answer_delta
        ),
    ];
    if let Some(ratio) = report.comparison.token_reduction_ratio {
        lines.push(format!("  token_reduction_ratio: {ratio:.4}"));
    }
    lines.join("\n")
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
