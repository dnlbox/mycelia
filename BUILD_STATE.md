# Build State

Agent working area. A fresh session reads this top to bottom, then follows
`prompt.md`. Keep `Now` shorter than one screen.

## Now

- Milestone: slice `21` distribution readiness SHIPPED. AGENTS.md and concept
  `21` are reconciled to curl-first plus official Homebrew/core later.
- Implemented: Cargo feature split. Default builds use `semantic-download`;
  Homebrew/system builds use `--no-default-features --features
  semantic-system-ort` with FastEmbed dynamic ORT loading and a direct `ort`
  initialization hook.
- Implemented: system-ORT runtime lookup. The formula wrapper sets
  `ORT_DYLIB_PATH` from Homebrew `onnxruntime`, and the CLI scans
  `HOMEBREW_PREFIX`, `/opt/homebrew`, and `/usr/local` when built for
  `semantic-system-ort`.
- Implemented: `install.sh` curl installer, README install notes, and doc scrub
  of local absolute paths and credential-source details.
- Validation: fmt, clippy, 90 tests, default release build, system-ORT release
  build, shell syntax, and lexical fixture smoke pass. Homebrew/core formula
  audit waits for the separate formula submission.
- Corpus: refreshed `forge` is 12,386 chunks, 12,386 embeddings, model
  `fastembed-5.17.2:BAAI/bge-small-en-v1.5`, db size 50.6 MB.
- Eval (68-case, refreshed Forge): routed default 52/68 @ 1413.7 tokens/answer.
- Publish gate: existing local history contains co-author trailers, so the safe
  GitHub publish path is a clean public history rather than pushing this full
  local development history.
- Blockers: None.

## Decisions

- 2026-06-25: Start with SQLite plus lexical retrieval because it proves the
  durable contracts without pretending the final embedding and vector design is
  known.
- 2026-06-25: Keep the first path synchronous and safe Rust until measurements
  justify concurrency or unsafe optimization.
- 2026-06-25: Invoke Cargo with the stable toolchain directory prepended to `PATH`
  because the Homebrew rustup shim omits Cargo subcommand discovery.
- 2026-06-25: Pin rusqlite 0.39 rather than enabling unstable compiler features;
  rusqlite 0.40's SQLite binding requires `cfg_select`, which Rust 1.92 lacks.
- 2026-06-25: Evict prior chunks when a changed source is rejected because stale
  retrieval violates the precision-first contract.
- 2026-06-25: Exclude the active database and SQLite sidecars from discovery so a
  database inside its corpus cannot index itself.
- 2026-06-25: Measure retrieval with sourced hit rate and MRR before selecting a
  semantic or full-text ranking implementation.
- 2026-06-25: Promote FTS5/BM25 to default after hit rate improved from 0.55 to
  0.80 and MRR from 0.308 to 0.541; keep substring as the reference adapter.
- 2026-06-25: Promote deterministic FTS5 reranking after reaching hit rate 1.00
  and MRR 0.688 with no regressions on the unchanged 20-case manifest.
- 2026-06-25: Exclude evaluation manifests from corpus discovery because indexing
  queries and expected sources contaminates retrieval measurements.
- 2026-06-25: Keep the reranker as default after the clean 40-case aggregate
  remained stronger, but treat its weights as provisional because raw FTS5 edged
  it on the new code-heavy cohort.
- 2026-06-25: Reject reciprocal-rank fusion after it regressed the 40-case
  baseline from 35 to 32 hits; do not add tree-sitter for retrieval quality while
  exact symbol queries already pass.
- 2026-06-25: Start MCP read-only over stdio with one launch-bound database;
  mutation tools wait for an explicit capability and consent design.
- 2026-06-25: Keep the official async MCP SDK and Tokio in `mycelia-cli`;
  `mycelia-core` remains synchronous and protocol-independent.
- 2026-06-25: Named corpus profiles store only canonical roots and derive database
  paths from validated names under local config and data directories.
- 2026-06-25: Move next to a bounded local semantic probe because lexical
  retrieval handles exact symbols but leaves measured paraphrase misses; keep
  brute-force vectors and defer production vector indexing.
- 2026-06-25: Use FastEmbed with `BAAI/bge-small-en-v1.5` for the probe because it
  provides local synchronous ONNX inference and a 384-dimensional retrieval model;
  isolate it behind a provider trait and use fakes in tests.
- 2026-06-25: Fix the indexing unit before more ranking work. Adopt tree-sitter
  for code chunk boundaries only (`09`), reversing the earlier no-tree-sitter
  stance for boundaries while keeping it out of ranking. Symbol chunks, then a
  distilled two-stage MCP surface (`10`), then a precision-first hybrid (`11`).
- 2026-06-25: Make tokens per answered query the primary retrieval gate alongside
  hit rate and MRR, because the product goal is consulting the index over
  re-reading files.
- 2026-06-26: Keep reranked FTS5 as default after the lexical-spine hybrid
  improved hit counts but failed the baseline token-per-answer gate.
- 2026-06-26: Do not adopt Graphify outright from the local AST bakeoff. It is
  mature graph prior art, but Mycelia beat it on the current code-only structural
  gate. Full Graphify document extraction requires an explicit model-spend gate.
- 2026-06-26: Keep `routed` as explicit opt-in (not default) until typed-edge
  work shows whether remaining misses are routing failures or structural missing
  context.
- 2026-06-26: Write each embedding batch to SQLite immediately after inference
  (not all at end) so interrupted runs resume rather than restart from zero.
- 2026-06-26: Add tree-sitter TypeScript and Python grammars; TS/Py structural
  chunking improved fts5-reranked eval more (34→47/68) than the routed strategy
  did alone, confirming chunk quality is the primary retrieval lever.
- 2026-06-26: Bump embedding batch size 128→256 and thread count to
  available_parallelism(); add live stderr progress counter.
- 2026-06-26: Resolve slice 13's deferred routed-default condition. Miss analysis
  showed remaining misses are recall/ranking, not structural missing context, so
  typed edges would not move the gate. Promote routed to CLI default and add a
  signature-line coverage signal (gated to identifier-shaped query terms) so
  symbol definitions outrank references. MCP and sync API stay on reranked FTS5
  pending a launch-loaded provider (slice 18).
- 2026-06-26: Route the stdio MCP server by default. Provider lives behind
  `Arc<Mutex<FastEmbedProvider>>` (router needs `Clone`, handlers take `&self`,
  `embed_query` needs `&mut`). Load the model once at startup; `serve --lexical`
  and graceful load-failure fallback keep embeddings non-mandatory. Sync API
  (`find`/`find_headers`) stays lexical (no provider).

- 2026-06-26 (REVIEWED, approved by user): The stdio MCP server self-heals its
  launch-bound index. On drift detected during `retrieve` (re-index/prune the
  touched file) and `find` (validate the top-K's sources, heal drift, re-rank
  once) it repairs via `refresh_source`. Rationale: the read-only-MCP decision
  governs the model-facing TOOL surface (no mutation tools, no arbitrary paths);
  a server maintaining its own bound database is internal upkeep, not a tool, so
  the surface is preserved. Smallest reversible default: heal only files the
  query already touched, never fail the call on a heal error, surface a manual
  `refresh` hint only when the index cannot be repaired. core read APIs
  (`retrieve`, `find_headers`, `drifted_sources`) stay pure-read; the server
  composes the write. CLI one-shot commands stay read-only.
- 2026-06-26: Freshness is precision, not refusal (user direction, supersedes the
  spec's refuse-on-stale). `retrieve` returns a tagged `Retrieved`
  (`ok`|`file`|`unavailable`) instead of `Option<ChunkRecord>`. Validate by
  re-reading and re-hashing the whole source file (no stored mtime to shortcut
  on; one file per call is negligible). Hash matches → return the precise indexed
  chunk. Hash differs → return the WHOLE current file read live, so the caller
  gets real up-to-date code and never needs to reason about a stale flag. Source
  removed, unreadable, escaped, or non-UTF-8 → `unavailable` signal. The consumer
  is never handed an indexed chunk whose source changed.
- 2026-06-26: License Mycelia under Apache-2.0 rather than MIT. The project is
  intended to be permissive FOSS and embeddable in agent tools, and Apache-2.0's
  explicit patent grant is the better default for that path.
- 2026-06-26: Do not create a personal Homebrew tap. Ship curl install first,
  then submit directly to Homebrew/core once acceptance constraints are met. The
  Homebrew/core formula must build from a tagged source archive and must not use
  the curl installer.

## Session log

- 2026-06-26: Distribution plan corrected per user direction: no tap, no
  source-repo formula; added curl installer, kept system-ORT feature split for
  future Homebrew/core, and bumped the public release target to v0.1.2 after the
  remote installer smoke caught the broken v0.1.1 Cargo syntax.
- 2026-06-26: Refreshed README to current shipped behavior and changed the repo
  license from MIT metadata-only to Apache-2.0 with a top-level license file.
- 2026-06-26: Slice 19 complete; shipped logs, stats/status, path-aware journey
  commands, setup/refresh/list/delete, and connect for Claude Code, Claude
  Desktop, Cursor, and Codex. 90 tests pass; release smoke and MCP exchange
  verified; refreshed Forge gate is routed 52/68.
- 2026-06-26: Freshness Layer 1 reframed to precision-over-caching per user;
  `retrieve` returns `ok` (precise chunk) / `file` (whole live file on change) /
  `unavailable`, never a stale slice. 79 tests pass; built-binary and live-Forge
  checks confirm fresh→ok and changed→whole-file.
- 2026-06-26: MCP server self-heals its bound index on drift (`refresh_source`),
  silently and without interrupting the flow; manual `refresh` is the last
  resort. Real stdio MCP exchange confirms find/retrieve converge to fresh. 83
  tests pass.
- 2026-06-26: Freshness Layer 1 completed; find validates the sources behind its
  top-K (`drifted_sources`), self-heals drift, and re-ranks once so headers are
  precise and agree with retrieve. User approved the self-heal decision. 85 tests
  pass; MCP exchange proves find→retrieve agreement after drift.
- 2026-06-25: Specialized the ai-protocol scaffold and locked the first milestone.
- 2026-06-25: First vertical index complete and verified against fixtures and Forge.
- 2026-06-25: Manifest-driven retrieval evaluator added; first Forge baseline
  captured with one intentional miss.
- 2026-06-25: Expanded the Forge baseline to 20 cases; measured substring
  retrieval at hit rate 0.55 and MRR 0.308.
- 2026-06-25: FTS5 migration and adapter verified; promoted after the measured
  20-case comparison.
- 2026-06-25: Phrase and token-coverage reranking recovered all raw FTS5 misses and
  became the default strategy.
- 2026-06-25: Expanded Forge evaluation to 40 cases and measured a clean,
  self-excluding corpus; reranking led overall while raw FTS5 led the new cohort.
- 2026-06-25: Tested and removed reciprocal-rank fusion; it recovered one hard
  code query but lost three established hits.
- 2026-06-25: Added and verified read-only stdio MCP `find` and `retrieve` tools
  against a real temporary index.
- 2026-06-25: Added named corpus profiles, installed the local binary, and built
  the derived Forge index without repository-specific client paths.
- 2026-06-25: Redirected the next substantive slice from MCP client configuration
  to the first measured semantic embedding probe.
- 2026-06-25: Completed the semantic probe; vectors improved the paraphrase cohort
  but failed the established precision gate, so reranked FTS5 remains default.
- 2026-06-25: Steered the plan into the protocol: ordered slices `09` code-aware
  chunking, `10` distilled MCP surface, `11` precision-first hybrid; aligned the
  vision/architecture/eval docs and reconciled Project Specifics.
- 2026-06-25: Slice 09 complete; tree-sitter-rust-v1 extractor promoted; 40-case
  baseline improved 34→35 hits; structural cohort added (5/8, exact symbols 4/4).
- 2026-06-25: Slice 10 complete; `find` now returns bounded headers, `retrieve`
  returns bodies, and eval reports token-per-answer estimates.
- 2026-06-26: Slice 11 complete; lexical-spine hybrid is selectable, but reranked
  FTS5 remains default because the hybrid failed the token-efficiency gate.
- 2026-06-26: Slice 12 complete; Graphify local AST bakeoff recorded and Mycelia
  kept as the active path pending any approved full-corpus Graphify run.
- 2026-06-26: Slice 13 complete; query-class routing added (`routed` strategy);
  routed=41/68 vs hybrid=37, fts5=34 on pre-TS/Py corpus.
- 2026-06-26: Slice 14 complete; incremental embedding writes; interrupted runs
  now resume from last committed batch.
- 2026-06-26: Slice 15 complete; TS/Py tree-sitter extraction; corpus 7411→9410
  chunks; fts5-reranked 34→47/68; 52 unit tests + 7 CLI tests pass.
- 2026-06-26: Slice 16 complete; embedding batch 128→256, all-CPU threads, live
  stderr progress; embed run observable and ~2× faster on cold start.
- 2026-06-26: Slice 17 complete; signature-coverage reranker signal lifted both
  strategies (fts5 47→48, routed 50→51) at lower token cost; routed promoted to
  CLI default with graceful lexical fallback; 66 tests pass.
- 2026-06-26: Slice 18 complete; stdio MCP server routes by default behind a
  shared embedding provider, with `--lexical` and load-failure fallback;
  retrieval surface feature-complete. Review checkpoint flagged before typed edges.
- 2026-06-26: Authored specs `19` (user journey/observability) and `20`
  (freshness/staleness). Decided freshness Layer 1 (query-time validation,
  refuse-on-stale) is the next implementation slice, ahead of journey work and
  typed edges, because returning stale code outranks tokenomics.
- 2026-06-26: Multi-agent review concluded; merged security/offline/perf/
  correctness fixes. Re-validated: 75 tests green, eval steady (routed 51/68,
  fts5 48/68) at slightly lower token cost. `Now` repointed to Freshness Layer 1.

## Archive

<!-- Completed milestones collapse to one line each; git holds full history. -->
