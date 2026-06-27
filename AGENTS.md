# AGENTS.md

The canonical, harness-agnostic contract for any AI coding agent in this repo.
Read this first, every session. Volatile state lives in `BUILD_STATE.md`;
human-authored intent lives in `docs/concept/`.

This file has two layers:

- Operational canonical: the universal guardrails, workflow, delegation, and
  continuity rules above the Project Specifics marker. Do not hand-edit them.
- Project Specifics: the concrete toolchain, validation gates, and Rust rules
  derived from `docs/concept/`.

## Agnostic tooling convention

- Persistent agent instructions live in `AGENTS.md`.
- Subdirectories may carry their own `AGENTS.md`; the deepest one wins.
- Skills, hooks, rules, and settings live under `.agents/`.
- Project skills are vendored and tracked in `skills-lock.json`.

## Source of truth

- `docs/concept/` defines what gets built. Read every file.
- The Project Specifics section below is derived from the concept documents.
- `BUILD_STATE.md` records current progress and handoff state.
- `prompt.md` is the static session kickstart.

## Operational constraints

- Never modify generated or compiled output.
- Never suppress compiler errors or lints with broad allowances.
- Never commit secrets, tokens, local databases, or environment values.
- Never leave debug output. Use structured command output or the project logger.
- Never expand scope. Every changed line must trace to the active slice.
- Run only commands declared in Project Specifics.
- Get approval before destructive or outward-facing actions.

## Validation workflow

Run the declared validation gates in order before completing a slice. Exercise
the behavior through the built binary, not only through unit tests.

## Delegation and model routing

- `deep`: architecture, security, integration, debugging, decisions, protocol
  files, and `BUILD_STATE.md`
- `standard`: bounded implementation against an explicit contract
- `fast`: mechanical edits, fixtures, and repetitive tests
- `explore`: read-only codebase and dependency questions

Parallelize within a slice, never across slices. Give each writer a disjoint file
area. Treat subagent output as untrusted until the main loop verifies it. Never
delegate decisions, protocol edits, state edits, or final integration.

## Session continuity protocol

1. Read this file, inventory `.agents/`, read `BUILD_STATE.md`, inspect the latest
   ten commits, and exercise the last checkpoint.
2. Before a slice, record its intent and verification plan in `BUILD_STATE.md`.
3. After a slice, verify it, replace `Now`, append one terse Session log line,
   and commit.
4. Never end on a broken tree. When context runs low, stop adding behavior, return
   to green, checkpoint, and write the next step.
5. For a new decision, choose the smallest reversible default, record it, and
   flag it for review.

<!-- BEGIN PROJECT SPECIFICS: reconciled from docs/concept/ by sync-protocols,
and hand-editable. Everything above is generic baseline; do not hand-edit it. -->

## Project Specifics

### Descriptor

`mycelia` is a local, content-agnostic knowledge index written in Rust. Its
implemented baseline is deterministic discovery, range-addressed UTF-8 chunks,
content-hash freshness, SQLite persistence, deterministic FTS5 reranking, raw
FTS5/BM25 and substring reference adapters, and manifest-driven retrieval
evaluation through a CLI. A read-only stdio MCP server exposes `find`, `retrieve`,
`search_codebase`, `locate_implementation`, and `list_corpora` against registered
local corpus profiles, resolving the corpus per request while preserving explicit
`--database` mode for fixtures and diagnostics. Named corpus profiles map
stable client-facing names to canonical roots and derived local database paths. A
measured local embedding adapter provides brute-force vector and lexical-spine
hybrid reference strategies. Query-class routing (`docs/concept/13`) is the CLI
default once a corpus is embedded, selecting a lexical-first blend for symbol
lookups and a semantic blend for prose; it falls back to reranked FTS5 when a
corpus has no embeddings, while reranked FTS5 stays the default for the
provider-less sync API. The stdio MCP server also routes by default behind a
shared lazy provider; `serve --lexical` or a model-load failure degrades it to
reranked FTS5 (`18`, `22`). The reranker rewards
signature-line coverage for identifier-shaped query terms so symbol definitions
outrank references (`17`) and collapses exact duplicate chunk bodies in limited
ranked headers so cloned boilerplate does not crowd out distinct candidates.
Code-aware tree-sitter
chunking (`docs/concept/09`), a distilled two-stage MCP surface measured in tokens
per answered query (`10`), and a precision-first hybrid re-measured over clean
chunks (`11`) are now in place. A local no-cost Graphify bakeoff (`12`) found
Graphify's AST graph valuable but not stronger than Mycelia on the code-only
structural gate; full-corpus Graphify evaluation requires explicit backend and
model-spend approval. Typed graph edges, MCP mutation tools, watchers, federation,
and specialized performance storage are deferred.

### Toolchain

Use the Rust toolchain by prepending its binary directory because this machine's
Homebrew `rustup` shim does not expose Cargo subcommands reliably.

| Action | Command |
| --- | --- |
| toolchain | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" rustc --version` |
| format | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt --all --check` |
| lint | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| test | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace --all-features` |
| build | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo build --workspace --release` |
| run | `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo run -p mycelia-cli --` |
| package manager | Cargo |

### Validation gates

Run these gates in order:

1. `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo fmt --all --check`
2. `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo clippy --workspace --all-targets --all-features -- -D warnings`
3. `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --workspace --all-features`
4. `env PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo build --workspace --release`
5. Run the documented CLI smoke test against a temporary fixture corpus.
6. Run the Forge retrieval manifest against a freshly updated Forge index.
7. Run a real stdio MCP exchange that initializes the server, lists `find` and
   `retrieve`, calls one tool against a temporary corpus, and closes cleanly.
8. Register and exercise a named corpus with isolated configuration and data
   homes.
9. For the semantic probe, compare lexical, vector, and hybrid retrieval on the
   unchanged Forge manifest plus the paraphrase cohort, and record embedding
   build time, refresh time, query latency, model identity, and storage size.

### Stack-specific rules

- Keep `mycelia-core` independent of CLI presentation and process exit behavior.
- Keep Tokio and MCP SDK types at the CLI transport boundary; `mycelia-core`
  remains synchronous and protocol-independent.
- In stdio MCP mode, reserve stdout exclusively for protocol messages. Diagnostics
  may use stderr.
- Bind an MCP server process to the local registered corpus profile store, or to
  one explicit database only in diagnostic `--database` mode. Tool arguments may
  select registered corpus names, but must never accept arbitrary database paths.
- Keep the MCP tool surface read-only until mutation capabilities and consent are
  explicitly designed: no model-facing mutation tools, no arbitrary database
  paths. The server may still self-heal the resolved corpus index as internal
  maintenance (re-index or prune a drifted file detected on `retrieve`); this is
  not a tool the model invokes and does not relax the surface.
- Store named corpus roots in local profile files and derive profile database
  paths from validated names; never store arbitrary database paths in profiles.
- Keep explicit path mode for fixtures and diagnostics, but never combine it with
  a named corpus target.
- Keep embedding providers behind a narrow core trait and record the model
  identifier and vector dimensions with cached embeddings.
- Use brute-force vector similarity for the first semantic probe; do not add an
  approximate vector index or specialized storage without measured need.
- Preserve independently selectable lexical and vector retrieval strategies while
  the semantic candidate is under evaluation.
- Select the extractor by content type behind the extractor trait. Borrow
  tree-sitter grammars for code chunking; never reimplement a parser. Code chunks
  are syntactic units (one per top-level symbol) carrying the signature and
  leading doc comment, with exact byte-range provenance. Unknown or non-code types
  keep plain-text chunking and a parse failure falls back to it, counted, never a
  panic.
- Bump the extractor identifier when chunk boundaries change so stale chunks and
  their embeddings invalidate; embed the symbol unit, not a raw byte slice.
- Exclude evaluation manifests from corpus discovery. They contain queries and
  expected sources, so indexing them contaminates measured retrieval quality.
- `find` returns distilled headers (path, line range, signature or synopsis,
  score, chunk id) under a result token budget; `retrieve` returns the full body.
  Gate retrieval changes on tokens per answered query, not only hit rate and MRR.
- `retrieve` re-validates the chunk against its source file on disk and yields a
  tagged `Retrieved` (`ok` | `file` | `unavailable`). Precision over caching:
  return the precise indexed chunk only when the file still hashes to the stored
  `source_hash` (`ok`); when the source changed, hand back the whole current file
  read live (`file`) so the answer is real and up to date, never a stale slice;
  when the source is gone, unreadable, or no longer text, return the
  `unavailable` signal. Never serve an indexed chunk whose source changed. This
  "do not lie to the model" guarantee is non-negotiable.
- When the server detects drift it self-heals its bound index via `refresh_source`
  (re-index a changed file, prune a gone/non-text one): `retrieve` heals the file
  it touched; `find` validates the sources behind its returned top-K
  (`drifted_sources`), heals drift, and re-ranks once so headers stay precise.
  Never fail the call on a heal error and never interrupt the flow to demand a
  manual `mycelia refresh`; only when the index cannot be repaired attach a
  last-resort `refresh_hint`. The filesystem is the single source of truth, so
  `find` and `retrieve` validate against it and must not contradict. Do not expose
  a "stale" flag on `find` headers: a re-ranked header is simply accurate.
#### Observability (slice `19`)

- Write one structured log line per meaningful event to
  `~/.local/share/mycelia/logs/<corpus-name>.log`. Format:
  `<YYYY-MM-DD HH:MM:SS>  <event>  <key=value ...>`. Events: `serve start`,
  `find`, `retrieve`.
- On every `find`, compute and append a token-savings estimate: actual tokens
  returned (sum of distilled header bytes ÷ 4) vs tokens-if-cold (sum of live
  source file sizes ÷ 4 — read the files, do not estimate from stored metadata).
  Record both figures in the log line.
- Rotate or cap log files to bound disk usage; exact threshold chosen at
  implementation time.
- `stats` reads the corpus log and prints aggregated query count, actual tokens,
  cold-read estimate, and savings ratio. No DB access required.
- `status` reads the DB (chunk count, embedding coverage, model identity,
  last-refresh timestamp, db size) and the log (last serve-start line) and reports
  health in plain text. When something is wrong, name the fix
  (`run mycelia refresh`).
- Log writes are best-effort: a log I/O failure must not surface as a command
  error or interrupt MCP tool handling.

#### Path-aware corpus resolution (slice `19`)

- For commands that accept no explicit `--corpus` or `--database`, walk up from
  cwd to the git root and match against registered corpus roots; the deepest
  registered root that contains cwd wins.
- `--corpus <name>` or `--database <path>` always override auto-resolution.
- `--database <path>` is for fixtures and diagnostics; never combine it with a
  named or inferred corpus target.
- When cwd is under no registered corpus, fail with a message naming the fix
  (`run mycelia setup`).
- Name collision (two corpora with the same basename): fail and ask for an
  explicit `--name`, rather than silently namespacing.

#### CLI surface (slice `19`)

- `setup [path] [--name N]`: register root + index + embed with live progress.
  Default root = git root of cwd; default name = basename of root. Idempotent:
  re-running is a refresh.
- `connect <harness>`: wire the corpus into the harness. Supported first-cut
  targets: `claude-code` (via `claude mcp add` CLI), `claude-desktop` (edit
  `~/Library/Application Support/Claude/claude_desktop_config.json`), `cursor`
  (edit `~/.cursor/mcp.json`), `codex` (edit `~/.codex/config.toml`). Emit the
  absolute path to the `mycelia` binary in the server spec. Idempotent: update the
  existing entry, never duplicate. Require registered corpus (error with a setup
  hint if not found). Verify exact config paths and CLI flags at implementation
  time; these tools move quickly.
- `stats`: tokenomics from the log (query count, actual tokens, cold-read
  estimate, savings ratio). Reads log only, no DB.
- `status`: health from DB + log (chunk count, embedding coverage, model identity,
  last refresh, db size). Names the fix when something is wrong.
- `refresh`: forced full re-index + embed. A user-triggered fallback; query-time
  freshness is the primary correctness mechanism.
- `list`: all registered corpora; mark the corpus matching cwd with `*`.
- `delete`: remove profile + database + embeddings + log after showing what will
  be removed and prompting for confirmation.
- Retire `corpus set`, `corpus show`, `corpus list`; their behaviour is absorbed
  by `setup`, `status`, and `list`.
- `serve` remains the internal harness-invoked command; do not promote it as a
  user-facing journey verb in help text.

#### Homebrew distribution (slice `21`)

- Default development builds use the `semantic-download` feature, which allows
  FastEmbed to download ORT binaries during the Rust build.
- Homebrew builds must use `--no-default-features --features
  semantic-system-ort`, depend on the package-manager `onnxruntime`, and avoid
  ORT build-time binary downloads.
- The Homebrew-installed binary must find ONNX Runtime without users exporting
  `ORT_DYLIB_PATH`. The formula wrapper may set it, and the CLI may scan standard
  Homebrew prefixes when built with `semantic-system-ort`.
- `setup` and `embed` are the only paths that may download the embedding model.
  Install, `find`, `serve`, and `connect` must not trigger model downloads or
  hidden corpus setup.
- Interim distribution is `cargo install --git`, `install.sh` via curl, and a
  personal tap for staging the Homebrew UX. The permanent Homebrew target is
  `homebrew/core`. Any Homebrew formula must build from a tagged source archive
  and must not shell out to the curl installer.

- Model source locations as byte ranges plus one-based line ranges.
- Make identifiers deterministic. Do not use random identifiers for indexed data.
- Keep extractors and retrieval strategies behind narrow traits.
- Treat invalid UTF-8 and unreadable files as counted rejections, not panics.
- Use forward-only numbered SQLite migrations.
- Keep FTS5 synchronized through migration-owned triggers on the `chunks` table.
- Keep SQL in the storage module and parameterize every value.
- Preserve deterministic ordering for discovery, persistence, and result ties.
- Avoid async, unsafe Rust, and performance-specific storage until measurements
  justify them.
- Add dependencies only when they remove meaningful implementation risk.

<!-- END PROJECT SPECIFICS -->
