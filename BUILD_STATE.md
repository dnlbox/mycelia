# Build State

Agent working area. A fresh session reads this top to bottom, then follows
`prompt.md`. Keep `Now` shorter than one screen.

## Now

- Latest shipped slice: concept `23`, typed graph edges, committed as
  `8d70340 feat: add typed graph edges`. It added migration 005
  (`chunks.symbol` + `edges`), conservative Rust depth-1 `calls` extraction,
  CLI `graph`, MCP `find_related`, graph coverage in `status`, forced
  `refresh` reindex, and schema upgrade on read.
- Verified checkpoint: fmt, clippy, 116 tests, release build, release-binary
  graph fixture smoke (including external/no-def names like `println` omitted),
  real stdio MCP exchange, isolated `setup`/`status`/`graph`/`connect codex`,
  self-indexed graph smoke, and fresh Forge refresh/eval. Current refreshed
  Forge gate: routed 50/68 at weighted 1391.9 tokens/answer; fts5-reranked
  48/68 at weighted 1395.9. No retrieval default change.
- V2 vision LOCKED and reconciled (2026-06-27): three planes (index, guidance,
  connection) divided by one consent boundary, the project boundary itself.
  Canonical spine is `docs/concept/v2/00_vision.md` with `consent-boundary.svg`.
  Reconciliation banners plus the reworded non-goal landed in `v2/01`, `v2/03`
  (new guidance-plane convention-detection subsection), `v2/README`, and
  `concept/24`. No CLI behavior changed.
- Execution plan written as `ROADMAP.md` (non-concept, repo root). Primary target
  is the interactive harness + MCP path (Mycelia + project + Codex, Claude Code,
  Antigravity, OpenCode, Kilo); headless CI is a kept secondary path and the clean
  corroborating measurement, not the bet. Phase A foundation
  (`.mycelia/config.toml` resolution + `mycelia init`) -> Phase B interactive path
  (guidance plane across the target harnesses + a per-harness organic-use A/B that
  is the publish-or-shelf gate) -> Phase C headless CI -> Phase D hardening ->
  Phase E library API.
- Latest shipped slice: Phase A / Slice A1, project config + cwd resolution.
  New `project.rs` with `ProjectConfig` (serde toml_edit) + `resolve_from_cwd`
  walk-up; resolution ladder in `CwdTarget`, `AnyTarget`, and MCP
  `resolve_corpus` (project-local → registry → error naming `mycelia init`);
  `log_path` on `ResolvedCorpus` in both `main.rs` and `mcp.rs`; `connect`
  emits `mycelia serve` (no `--corpus`) for project-local; MCP `instructions`
  lists project corpus first. Verified: temp repo with hand-written
  `.mycelia/config.toml` resolves for status/find/serve with no registry entry;
  legacy registry and explicit flags still work; 120 tests; release build;
  project-local MCP exchange with correct corpus namespacing.
- Latest shipped slice: Phase A / Slice A2, `mycelia init`. Creates
  `.mycelia/` tree (config.toml, AGENTS.md fragment, .gitignore, db/logs/cache
  dirs); indexes into `.mycelia/db/index.sqlite3` via existing pipeline;
  consent-gated one-line owned block into any root AGENTS.md or CLAUDE.md
  (idempotent replace-in-place, never duplicates). Verified: tree created,
  config.toml not overwritten on re-run, guidance block applied/updated on y /
  skipped on n; `mycelia status` from project cwd resolves via config.toml
  alone; 131 tests; fmt/clippy clean; release build.
- Next implementation slice: Phase B / Slice B1, guidance plane. Per harness
  (Codex, Claude Code), detect and wire a consent-gated, idempotent, removable
  owned block into the harness instruction convention. For Claude Code, also
  write a project `.claude/settings.json` eager tool-load entry.
- Blockers: none.

## Decisions

- 2026-06-27: Primary v2 target is the interactive harness + MCP path (Mycelia +
  project + Codex, Claude Code, Antigravity, OpenCode, Kilo), and the
  publish-or-shelf gate is measured there: organic Mycelia use plus
  tokens-to-right-files vs the grep baseline, per harness. Headless CI stays as a
  secondary path and the clean corroborating measurement, not the bet. This makes
  the guidance plane make-or-break, so `ROADMAP.md` promotes it to the primary
  Phase B and demotes CI to Phase C.
- 2026-06-27: V2 vision locked as three planes (index, guidance, connection)
  divided by one consent boundary, the project boundary itself. Inside the repo
  Mycelia may integrate aggressively when each change is committed, idempotent,
  removable, and previewed; outside the repo there is exactly one touch,
  `connect`. "Non-invasive" means nothing hidden and nothing machine-level beyond
  the one server, not "do not touch instruction files." Spine:
  `docs/concept/v2/00_vision.md`.
- 2026-06-27: `connect` is a per-harness-install action (one global server, cwd
  self-discovery, a single entry in harness settings); `init` is a per-clone
  action. The "repo carries everything but connection is per-developer" seam is a
  lifecycle distinction, not a contradiction. Rejected repo-carried `.mcp.json`
  (one MCP entry per project conflicts with the one-server UX).
- 2026-06-27: Sequence v2 to reach a measured headless-agent A/B (Phase B in
  `ROADMAP.md`) as the publish-or-shelf gate before building artifacts, full
  convention detection, or the library API.
- 2026-06-27: Treat Mycelia dogfooding as a product gate, not a moral request.
  Every Mycelia slice should either show recent Mycelia `find`/`retrieve` use or
  explicitly explain why direct shell/source reads were the better path.
- 2026-06-27: Keep `stats`, not a new `doctor adoption` command, as the primary
  user-facing adoption surface. Add `stats --all` before adding another top-level
  command.
- 2026-06-27: Do not force MCP use through hard hooks. Prefer transparent
  harness guidance and optional soft nudges; if project instruction files are
  written, they must be visible, idempotent, and easy for the user to approve or
  remove.
- 2026-06-27: Separate CLI surfaces by audience. User journey verbs are
  `setup`, `connect`, `stats`, `status`, `refresh`, `list`, and `delete`;
  diagnostic/manual verbs such as `find`, `retrieve`, `graph`, `eval`, `embed`,
  `index`, and `serve` may remain for tests and power use but should not dominate
  onboarding help.
- 2026-06-27: V2 pivots from user-level harness configuration to project-level
  self-discovery. Default writes live under `.mycelia/`; writes outside that
  boundary require preview and confirmation. CI/headless agents become a primary
  adoption path, not an afterthought.

## Session log

- 2026-06-27: Retrospective slice defined concept `24`, compacted
  `BUILD_STATE.md`, synced Project Specifics, and propagated trigger-based
  state consolidation plus sync-protocol closeout gates into `ai-protocol`.
- 2026-06-27: Captured v2 rewrite docs under `docs/concept/v2/`: project-local
  `.mycelia/` layout, local/team/CI/library journeys, CI headless PR-agent flow,
  indexer speed/artifact strategy, visibility diagnostics, and carry-forward
  requirements from v1.
- 2026-06-27: Reconciled the v2 docs around a three-plane / consent-boundary
  spine (new `00_vision.md` + `consent-boundary.svg`; banners and fixes in
  `01`/`03`/`24`/`README`) and wrote `ROADMAP.md` sequencing foundation ->
  provable wedge -> hardening -> library with an explicit publish-or-shelf gate.
- 2026-06-28: Phase A / Slice A1 — project config resolution shipped. New
  `project.rs`; resolution ladder across `main.rs` and `mcp.rs`; 120 tests.
- 2026-06-28: Phase A / Slice A2 — `mycelia init` shipped. Tree creation,
  `--no-embed`, consent-gated guidance include, idempotent; 131 tests.

## Archive

- 2026-06-25 to 2026-06-26: Built the deterministic SQLite/FTS5 baseline, added
  code-aware chunking, distilled `find`/`retrieve`, semantic/vector probes,
  routed retrieval, MCP stdio, freshness/self-heal, named profiles, and journey
  commands. Old detailed metrics live in git history and concept docs.
- 2026-06-26 to 2026-06-27: Shipped distribution readiness, install docs,
  progress polish, MCP adoption descriptions and aliases, Ruby extraction,
  multi-corpus MCP, eval-manifest exclusion, duplicate-body header compaction,
  and the first typed graph slice.
