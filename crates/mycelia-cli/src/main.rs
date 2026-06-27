mod log;
mod mcp;
mod profile;
mod semantic;

use clap::{Args, Parser, Subcommand, ValueEnum};
use mycelia_core::{
    self, ChunkRecord, EmbeddingReport, EvaluationCase, EvaluationReport, IndexReport,
    RetrievalStrategy, Retrieved, SearchHeader,
};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

type Result<T> = std::result::Result<T, String>;

const MISSING_EMBEDDINGS_HINT: &str = "corpus has no embeddings for this model; run `mycelia embed` first or pass `--strategy fts5-reranked`";

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
    Connect {
        /// Target harness: claude-code, claude-desktop, cursor, codex.
        harness: String,
        #[command(flatten)]
        target: CwdTarget,
    },
    /// Show token-savings statistics aggregated from the activity log.
    Stats {
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
    /// Run a manifest-driven retrieval evaluation (power use).
    Eval {
        manifest: PathBuf,
        #[command(flatten)]
        target: AnyTarget,
        #[arg(long, value_enum, default_value_t)]
        strategy: Strategy,
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
        /// Serve lexical-only (reranked FTS5), skipping the embedding model load.
        #[arg(long)]
        lexical: bool,
    },
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
}

impl CwdTarget {
    fn resolve(self) -> Result<ResolvedCorpus> {
        match (self.corpus, self.database) {
            (Some(name), None) => {
                let p = profile::get(&name)?;
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                })
            }
            (None, Some(db)) => Ok(ResolvedCorpus {
                database: db,
                corpus_name: None,
                corpus_root: None,
            }),
            (None, None) => {
                let cwd = std::env::current_dir()
                    .map_err(|error| format!("cannot determine current directory: {error}"))?;
                let p = profile::infer_from_cwd(&cwd)?;
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                })
            }
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
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                })
            }
            (None, Some(db)) => Ok(ResolvedCorpus {
                database: db,
                corpus_name: None,
                corpus_root: None,
            }),
            (None, None) => {
                let cwd = std::env::current_dir()
                    .map_err(|error| format!("cannot determine current directory: {error}"))?;
                let p = profile::infer_from_cwd(&cwd)?;
                Ok(ResolvedCorpus {
                    database: p.database,
                    corpus_name: Some(p.name),
                    corpus_root: Some(p.root),
                })
            }
            (Some(_), Some(_)) => {
                Err("--corpus and --database cannot be used together".to_string())
            }
        }
    }

    fn resolve_database(self) -> Result<PathBuf> {
        Ok(self.resolve()?.database)
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

// Main dispatch

#[derive(Deserialize)]
struct EvaluationManifest {
    limit: usize,
    cases: Vec<EvaluationCase>,
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
        Command::Setup {
            path,
            name,
            no_embed,
        } => cmd_setup(path, name, no_embed),
        Command::Connect { harness, target } => cmd_connect(&harness, target),
        Command::Stats { target } => cmd_stats(target),
        Command::Status { target } => cmd_status(target),
        Command::Refresh { target } => cmd_refresh(target),
        Command::List => cmd_list(),
        Command::Delete { target } => cmd_delete(target),
        Command::Corpus => Err(
            "`mycelia corpus` has been retired. Use `mycelia setup`, `mycelia status`, or `mycelia list`."
                .to_string(),
        ),

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

        Command::Eval {
            manifest,
            target,
            strategy,
            json,
        } => {
            let resolved = target.resolve()?;
            let contents = fs::read_to_string(&manifest)
                .map_err(|e| format!("failed to read {}: {e}", manifest.display()))?;
            let manifest: EvaluationManifest = serde_json::from_str(&contents)
                .map_err(|e| format!("invalid evaluation manifest: {e}"))?;
            let report = eval_with_strategy(
                &resolved.database,
                &manifest.cases,
                manifest.limit,
                strategy,
            )?;
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
            let mut provider =
                semantic::FastEmbedProvider::prepare(&database).map_err(|e| e.to_string())?;
            let report = mycelia_core::refresh_embeddings(&database, &mut provider)
                .map_err(|e| e.to_string())?;
            emit_output(if json {
                serde_json::to_string(&report).map_err(|e| e.to_string())?
            } else {
                format_embedding_report(&report)
            });
            Ok(())
        }

        Command::Serve { target, lexical } => {
            let resolved = target.resolve()?;
            mcp::serve(
                resolved.database,
                resolved.corpus_name,
                resolved.corpus_root,
                lexical,
            )
        }
    }
}

// Journey commands

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
        eprintln!("Embedding...");
        let mut provider =
            semantic::FastEmbedProvider::prepare(&profile.database).map_err(|e| e.to_string())?;
        mycelia_core::refresh_embeddings(&profile.database, &mut provider)
            .map_err(|e| e.to_string())?;
    }

    eprintln!("Done. Run `mycelia connect <harness>` to wire it into your AI tool.");
    Ok(())
}

fn cmd_connect(harness: &str, target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.ok_or_else(|| {
        "connect requires a named corpus; use --corpus or run from a registered directory"
            .to_string()
    })?;

    let binary =
        std::env::current_exe().map_err(|e| format!("failed to determine binary path: {e}"))?;
    let binary_str = binary
        .to_str()
        .ok_or_else(|| "binary path contains non-UTF-8 characters".to_string())?
        .to_owned();

    let server_name = format!("mycelia-{corpus_name}");
    let args = ["serve", "--corpus", &corpus_name];

    match harness {
        "claude-code" => connect_claude_code(&server_name, &binary_str, &args),
        "claude-desktop" => connect_json_file(
            &server_name,
            &binary_str,
            &args,
            claude_desktop_config_path()?,
            "Claude Desktop",
        ),
        "cursor" => connect_json_file(
            &server_name,
            &binary_str,
            &args,
            cursor_config_path()?,
            "Cursor",
        ),
        "codex" => connect_codex(&server_name, &binary_str, &args),
        other => Err(format!(
            "unknown harness '{other}'; supported: claude-code, claude-desktop, cursor, codex"
        )),
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
        serde_json::from_str(&text)
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

fn cmd_stats(target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("<unnamed>");
    let log_path = resolved
        .corpus_name
        .as_deref()
        .and_then(|name| profile::log_path_for(name).ok());

    let stats = log_path.as_deref().map(log::read_stats).unwrap_or_default();

    println!("corpus:            {corpus_name}");
    println!("queries answered:  {}", stats.queries);

    if stats.queries == 0 {
        println!("(no find queries logged yet. Start a serve session to accumulate data)");
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
    Ok(())
}

fn cmd_status(target: CwdTarget) -> Result<()> {
    let resolved = target.resolve()?;
    let corpus_name = resolved.corpus_name.as_deref().unwrap_or("<unnamed>");

    let db_stats = mycelia_core::corpus_status(&resolved.database).map_err(|e| e.to_string())?;

    let log_path = resolved
        .corpus_name
        .as_deref()
        .and_then(|name| profile::log_path_for(name).ok());

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

    println!("corpus:            {corpus_name}");
    println!("index:             {} chunks", db_stats.chunk_count);
    println!("embeddings:        {embedding_line}");
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
        mycelia_core::index_corpus(&root, &resolved.database).map_err(|e| e.to_string())?;
    eprintln!(
        "  {} chunks from {} files ({} removed, {} rejected)",
        report.chunks_written, report.indexed, report.removed, report.rejected
    );
    eprintln!("Embedding...");
    let mut provider =
        semantic::FastEmbedProvider::prepare(&resolved.database).map_err(|e| e.to_string())?;
    mycelia_core::refresh_embeddings(&resolved.database, &mut provider)
        .map_err(|e| e.to_string())?;
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
    let inferred_name = cwd
        .as_deref()
        .and_then(|cwd| profile::infer_from_cwd(cwd).ok())
        .map(|p| p.name);

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

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
