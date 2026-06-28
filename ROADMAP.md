# Mycelia v2 execution roadmap

This is the execution plan from the current shipped code to the v2 vision, plus
the decision gate that tells us whether to publish v2 or shelf the project. It is
deliberately not a concept document. The vision it builds toward is
`docs/concept/v2/00_vision.md`; the detail behind each plane lives in the rest of
`docs/concept/v2/`. Volatile day-to-day state stays in `BUILD_STATE.md`.

Read order for a fresh agent: `AGENTS.md`, then `docs/concept/v2/00_vision.md`,
then this file, then `BUILD_STATE.md`.

## The single goal

Reach a measurable answer to one question as cheaply as possible: does Mycelia,
attached to a project, make a developer's real interactive sessions meaningfully
better, across the harnesses we actually use? The primary path is the local
harness plus MCP, with Mycelia discovered from the project, on Codex, Claude Code,
Antigravity, OpenCode, and Kilo. Success means the model organically reaches for
`find` for orientation instead of grep, and reaches the right files for fewer
tokens. Headless CI is a viable and valuable secondary path, and the cleanest
measurement environment, but it is not the bet.

This is the harder path to win and to measure, because interactively Mycelia
competes against an always-available grep reflex and a sometimes-deferred MCP
schema. That is the point: it is what gets used, so the sequence front-loads the
foundation and the guidance plane that makes the interactive path get used, and
defers most hardening and the library API until that path has proven itself.

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

The roadmap has one explicit go/no-go, at the end of Phase B, measured on the
primary interactive path. Phases C onward do not start until Phase B answers yes.
If Phase B answers no, the deliverable is a written result and a recommendation to
shelf or to narrow scope, not more code.

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

## Phase B: the interactive path (the bet)

Goal of the phase: across the harnesses we actually use, a developer working in a
project-attached repository organically reaches for Mycelia for orientation
instead of grep, and reaches the right files for fewer tokens. This is the primary
product target and the publish-or-shelf gate.

Target harnesses, in priority order: Codex and Claude Code first (both already
have `connect` support), then Antigravity, OpenCode, and Kilo. Both `connect` and
the guidance plane grow to cover them. Resolve each harness's exact MCP config and
instruction convention at implementation time, since these tools move quickly and
the newer three are not yet `connect` targets.

### Slice B1: guidance plane across the target harnesses

- Goal: make the "use Mycelia first" guidance and the loaded index actually reach
  the model in each target harness, through the convention that harness already
  reads.
- Steps:
  1. Per harness, detect and wire a consent-gated, idempotent, removable owned
     block into its instruction convention: `AGENTS.md`/`CLAUDE.md`, Codex project
     config, Antigravity rules, OpenCode `AGENTS.md`/config, Kilo rules.
  2. For Claude Code, also write the project `.claude/settings.json` eager
     tool-load so the MCP schema is not deferred and lost to grep.
  3. Extend `connect` to the harnesses it does not yet support (Antigravity,
     OpenCode, Kilo), keeping one generic server per harness.
- Touches: `main.rs` (`connect` targets, `init` guidance writers), a new guidance
  module, per-harness config writers.
- Verify: in an isolated temp project, each harness's owned block writes once,
  updates in place, and removes cleanly; standard gates; a real stdio MCP exchange
  stays clean.
- Exit: for each target harness, a connected developer in a project-attached repo
  has both the guidance and the loaded tool in front of the model.

### Slice B2: the interactive measurement (the decision gate)

- Goal: measure organic Mycelia use and token cost on the primary path, per
  harness, against the grep-reflex baseline.
- Steps:
  1. Build a small set of realistic orientation and implementation tasks on a real
     repository with known-good touched files.
  2. Run paired sessions per harness: guidance-installed-and-loaded vs baseline.
     Read transcript tool-call counts (Mycelia `find`/`retrieve` vs grep/read),
     tokens spent before the right files, files touched, and the `mycelia stats`
     delta to confirm organic use.
  3. Start with Codex and Claude Code; add the others as B1 lands them.
- Touches: a measurement harness under the scratchpad or `fixtures/`, plus a
  results write-up. No production code.
- Decision: if, with the guidance plane installed, the model organically leads
  with Mycelia and reaches the right files for fewer tokens than baseline across
  the primary harnesses, proceed. Otherwise stop and write the shelf-or-narrow
  recommendation. This is the publish-or-shelf gate.

## Phase C: the headless path (secondary, and the clean measurement)

Goal of the phase: support headless CI agents and use their deterministic
environment as a clean corroborating measurement. Valuable, and a real adoption
path for teams, but not the bet.

### Slice C1: `mycelia ci seed-context`

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

### Slice C2: `mycelia ci prepare`

- Goal: resolve and refresh the project index and emit environment plus a
  machine summary for a headless agent.
- Steps:
  1. Locate `.mycelia/config.toml`, build or refresh the index (a full refresh is
     acceptable first; git-diff-aware incremental refresh is a later optimization
     in Phase D).
  2. Emit `.mycelia/ci.env` and a summary JSON (`index_ready`, `files_refreshed`,
     `mcp_command`, `warnings`).
  3. Exit non-zero when the index cannot be trusted.
- Touches: `ci.rs`, `main.rs`.
- Verify: prepare in a temp repo emits env and summary, fails clearly on an
  untrustworthy index; standard gates.
- Exit: a CI job can prepare, then seed, then serve from the emitted env.

### Slice C3: corroborating CI A/B

- Goal: in the deterministic CI environment, measure a headless agent with and
  without seed-context (tokens to first patch, first-file-hit, validation pass,
  wall time) on a repository with known-good touched files.
- Use: corroborate the Phase B interactive finding with a clean number; it does
  not replace the interactive gate.

## Phase D: team hardening

### Slice D1: project-default observability and CLI cleanup

- `stats` and `status` default to the resolved `.mycelia/` project; `stats --all`
  spans project-local and legacy registered indexes; clearer zero-use language;
  retire the `corpus` group from visible help; keep the dogfood gate wording.
  See `docs/concept/24_dogfood_and_protocol_adoption.md` and `v2/06`.

### Slice D2: artifact lifecycle and CI cache

- `artifact export/import/verify` with a manifest; CI cache keys; main-branch
  artifact publish; PR restore plus git-diff-aware incremental refresh. See
  `v2/05`.

## Phase E: long-term moat (deferred)

### Slice E1: stable `mycelia-core` library API

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
