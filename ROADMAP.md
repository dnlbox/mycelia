# Mycelia v2 execution roadmap

This is the execution plan from the current shipped code to the v2 vision, plus
the decision gate that tells us whether to publish v2 or shelf the project. It is
deliberately not a concept document. The vision it builds toward is
`docs/concept/v2/00_vision.md`; the detail behind each plane lives in the rest of
`docs/concept/v2/`. Volatile day-to-day state stays in `BUILD_STATE.md`.

Read order for a fresh agent: `AGENTS.md`, then `docs/concept/v2/00_vision.md`,
then this file, then `BUILD_STATE.md`.

## The single goal

Reach a measurable answer to one question as cheaply as possible: does
project-attached Mycelia make a headless agent meaningfully better (fewer tokens
to first patch, more accurate first files touched) on a real repository? Every
slice before that answer exists to make the measurement possible. Everything
after it is conditional on the answer being yes.

This is why the sequence front-loads the foundation and the CI wedge, and defers
artifacts, full convention detection, and the library API until the wedge has
proven itself.

## Where we are now (the starting line)

Shipped and measured (see `BUILD_STATE.md`, `README.md`):

- Retrieval: deterministic chunking (Rust, TS, TSX, Python, Ruby), reranked
  FTS5, vector, hybrid, and routed strategies; routed default at 50/68 on the
  Forge gate at ~1392 tokens/answer. Query-time freshness and self-heal.
- Graph: conservative depth-1 Rust `calls` edges, `graph` CLI, `find_related` MCP.
- MCP: one read-only multi-corpus stdio server (`find`, `retrieve`, aliases,
  `find_related`, `list_corpora`).
- CLI journey: `setup`, `connect` (codex, claude-code, claude-desktop, cursor),
  `stats`, `status`, `refresh`, `list`, `delete`, plus diagnostic verbs.
- Distribution: Cargo/curl install now, Homebrew path staged.

The resolution model today is user-level, and this is the main thing v2 changes:

- Corpus roots are stored in `~/.config/mycelia/corpora/<name>.json`; databases
  in `~/.local/share/mycelia/corpora/<name>.sqlite3`
  (`crates/mycelia-cli/src/profile.rs`).
- `infer_from_cwd` resolves a corpus by matching cwd against that user-level
  registry, deepest registered root wins.
- There is no `.mycelia/` project layout and no `init` command.

So the v2 code shift is: introduce `.mycelia/config.toml` as an in-repo
resolution source that the journey commands prefer, with the existing
user-level registry kept only as fallback and migration.

## Carry-forward invariants (every slice must preserve)

These are non-negotiable and predate v2. A slice that breaks one is not done.

- Two-stage `find`/`retrieve` under a token-per-answer gate. Hit-rate gains that
  cost more answer tokens are not wins.
- Never serve a stale slice. `retrieve` returns the precise chunk only when the
  source still hashes; otherwise the whole live file, or `unavailable`.
- Deterministic discovery, chunk identity, and result ordering.
- Read-only MCP surface. No model-facing mutation tools, no arbitrary database
  paths from the model.
- Eval manifests excluded from discovery.
- Writes outside `<project>/.mycelia/` are previewed and confirmed; the default
  answer is no. Owned blocks are idempotent and removable.
- `connect` is the only action that writes user-level (machine) config.

## Decision gate

The roadmap has one explicit go/no-go, at the end of Phase B. Phases C and D do
not start until Phase B answers yes. If Phase B answers no, the deliverable is a
written result and a recommendation to shelf or to narrow scope, not more code.

## Phase A: the project contract

Goal of the phase: a freshly cloned repository can be discovered and served from
its own `.mycelia/`, with zero user-level setup beyond having the binary.

### Slice A1: project config and resolution

- Goal: make `.mycelia/config.toml` an in-repo resolution source, preferred over
  the user-level registry.
- Steps:
  1. Define a `ProjectConfig` (serde) for `.mycelia/config.toml`: project name,
     root, index database path under `.mycelia/db/`, discovery policy
     (gitignore, excludes), instructions file. New
     `crates/mycelia-cli/src/project.rs`.
  2. Add `resolve_project_from_cwd(cwd)` that walks up to the nearest
     `.mycelia/config.toml` and returns the resolved root and database path.
  3. New resolution ladder for `CwdTarget` and `AnyTarget`: explicit
     `--database`/`--corpus` first, then `.mycelia/config.toml` walk-up, then the
     legacy `infer_from_cwd` registry, then an error that names `mycelia init`.
- Touches: new `project.rs`; target resolution in `main.rs`; call order in
  `profile.rs`. Core storage is unaffected (the database is just a path).
- Verify: a temp repo with a hand-written `.mycelia/config.toml` resolves for
  `find`/`status`/`serve` with no registry entry; legacy registry still resolves
  when no `.mycelia/` is present; explicit flags still override; standard gates.
- Exit: `mycelia status` answers from `.mycelia/config.toml` alone.

### Slice A2: `mycelia init`

- Goal: create and maintain `.mycelia/`, consent-gated for anything outside it.
- Steps:
  1. `Init` command writes `.mycelia/config.toml`, the `.mycelia/AGENTS.md`
     source fragment, the `db/ logs/ cache/` directories, and a
     `.mycelia/.gitignore` that ignores `db/ logs/ cache/ artifacts/`.
  2. Index and embed into `.mycelia/db/index.sqlite3`, reusing the `setup`
     pipeline; keep a `--no-embed` flag for offline and tests.
  3. Minimal guidance cut: detect a root `AGENTS.md` or `CLAUDE.md` and preview a
     one-line include of `.mycelia/AGENTS.md`; apply only on confirmation. Full
     convention detection is Phase C.
  4. Idempotent: re-running updates the owned block, never duplicates; any write
     outside `.mycelia/` always previews first.
- Touches: `main.rs` (`Init`), `project.rs` (writers), the existing index/embed
  pipeline.
- Verify: init in a temp repo creates the tree and is idempotent; declining the
  root include still yields a working project; standard gates plus a CLI smoke.
- Exit: `git clone && mycelia init && mycelia serve` works, and nothing outside
  `.mycelia/` is written unless the user confirms.

## Phase B: the provable wedge

Goal of the phase: produce the publish-or-shelf evidence. Build the smallest
thing that lets a headless agent start from sourced orientation instead of broad
reading, then measure it.

### Slice B1: `mycelia ci seed-context`

- Goal: turn issue or ticket text into a compact, sourced orientation pack built
  from `find` headers, not raw file reads. Must run lexical or graph-only so CI
  needs no model download.
- Steps:
  1. New `ci` command group with a `seed-context` subcommand
     (`--issue-file`, `--json`).
  2. Resolve the project (A1), derive queries from the issue text, run `find`
     and graph neighbours, and assemble `likely_files`, `likely_symbols`,
     `recommended_queries`, `tests_to_check`, and `warnings` from headers.
  3. Respect the token budget; warn when the project is unindexed or stale.
- Touches: new `ci.rs`, `main.rs` (`ci` group), reuse of core `find` and graph.
- Verify: seed-context over a fixture issue yields sourced JSON, mutates nothing,
  and works without embeddings; standard gates.
- Exit: one command turns an issue file into a sourced orientation pack.

### Slice B2: `mycelia ci prepare`

- Goal: resolve and refresh the project index and emit environment plus a
  machine summary for a headless agent.
- Steps:
  1. Locate `.mycelia/config.toml`, build or refresh the index (a full refresh is
     acceptable first; git-diff-aware incremental refresh is a later optimization
     in Phase C).
  2. Emit `.mycelia/ci.env` and a summary JSON (`index_ready`, `files_refreshed`,
     `mcp_command`, `warnings`).
  3. Exit non-zero when the index cannot be trusted.
- Touches: `ci.rs`, `main.rs`.
- Verify: prepare in a temp repo emits env and summary, fails clearly on an
  untrustworthy index; standard gates.
- Exit: a CI job can prepare, then seed, then serve from the emitted env.

### Slice B3: the measured A/B (the decision gate)

- Goal: on one real repository and a handful of real issues, measure a headless
  agent with and without seed-context.
- Steps:
  1. Pick a repository and 5 to 10 issues whose correct touched files are known.
  2. Run paired headless sessions (seed-context vs none) under a fixed model and
     config.
  3. Record tokens to first patch, first-file-hit accuracy, validation pass rate,
     and wall time. Write up the result.
- Touches: a measurement harness under the scratchpad or `fixtures/`, plus a
  results write-up. No production code.
- Verify: the paired runs are reproducible and the metrics are recorded.
- Decision: if seed-context measurably reduces tokens to first patch and raises
  first-file-hit on the sample, proceed to Phase C. Otherwise, stop and write the
  shelf-or-narrow recommendation. This is the publish-or-shelf gate.

## Phase C: team hardening (only if Phase B says yes)

### Slice C1: guidance-plane convention detection

- Full detection and consent-gated, idempotent, removable owned blocks for
  `AGENTS.md`, `CLAUDE.md`, Cursor `.cursor/rules/*.mdc`, Codex project config,
  Antigravity, and Claude Code `.claude/settings.json` (including eager tool
  loading so a deferred MCP schema does not lose to grep). Every path is previewed
  before writing. See `docs/concept/v2/00_vision.md` and `03`.

### Slice C2: project-default observability and CLI cleanup

- `stats` and `status` default to the resolved `.mycelia/` project; `stats --all`
  spans project-local and legacy registered indexes; clearer zero-use language;
  retire the `corpus` group from visible help; keep the dogfood gate wording.
  See `docs/concept/24_dogfood_and_protocol_adoption.md` and `v2/06`.

### Slice C3: artifact lifecycle and CI cache

- `artifact export/import/verify` with a manifest; CI cache keys; main-branch
  artifact publish; PR restore plus git-diff-aware incremental refresh. See
  `v2/05`.

## Phase D: long-term moat (deferred)

### Slice D1: stable `mycelia-core` library API

- Resolve-from-cwd plus `find`/`retrieve`/graph behind a stable contract with no
  CLI or MCP assumptions, so a harness can embed Mycelia directly (journey E).
  This is the strongest long-term position, but it is deferred until after the
  publish decision because it is the most speculative and the least measurable.

## Per-slice process

Each implementation slice follows the existing repo discipline: record intent and
a verification plan in `BUILD_STATE.md` first; keep every changed line traceable
to the active slice; run the standard gates (`fmt`, `clippy`, `test`, release
build) plus the relevant CLI and MCP smokes in `AGENTS.md`; close with dogfood
evidence (`mycelia stats ... --recent`), or state why direct reads were the right
path; end on a green tree and commit.

## Out of scope for v2

- Ranking or retrieval-quality changes beyond the carry-forward gate.
- Hard command interception or hooks that block grep, read, or edit.
- Model-facing mutation MCP tools.
- Committed binary indexes by default.
- Federation and specialized vector or storage layers, until local measurements
  justify them.
