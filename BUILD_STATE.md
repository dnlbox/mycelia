# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 0 — Measurement baseline & determinism proof
- **Slice:** Phase 0 / Slice 2 complete
- **Status:** ready for next Phase 0 slice
- **Tree:** green (2026-06-29: fmt, clippy, tests, release build, CLI smoke, paired eval run, MCP smoke, stats)

## Next up

Phase 0, third item: verify the lexical-only path (R6) and deterministic IDs (R3). This should produce the remaining GO/NO-GO 0 evidence, then stop for lead review instead of starting Phase 1.

## Gate status

- [ ] **GO/NO-GO 0** — determinism + measurement baseline (awaiting Phase 0 slices)
- [ ] GO/NO-GO 1 — per-commit index + CI artifact
- [ ] GO/NO-GO 2 — change-scoped retrieval
- [ ] GO/NO-GO 3 — Vercel AI SDK 7.0 integration
- [ ] GO/NO-GO 4 — SHIP

## Done log (append-only, terse — newest last)

- 2026-06-29 — v1 reset: docs + roadmap + build-loop established; engine inherited from `main` (tree-sitter chunking for Rust/TS/TSX/Python/Ruby, SQLite schema v5, deterministic chunk IDs, freshness, read-only MCP, Rust calls graph). Nothing built against the v1 roadmap yet.
- 2026-06-29 — Phase 0 / Slice 1: added v1 fixed-task eval schema (`required_files`), emitted required files in eval results, rejected in-corpus eval manifests at runtime, added `fixtures/eval/mycelia-v1-code.json` with five code-only tasks, and documented the manifest contract. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; eval run 5/5 hits, MRR 0.8000, tokens/answer 1219.8; MCP smoke ok; stats ok.
- 2026-06-29 — Phase 0 / Slice 2: added `mycelia eval --paired` with a deterministic live-file `grep_read` baseline, paired JSON/text output (`mycelia`, `baseline`, `comparison`), comparison deltas, and tests. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; paired eval run on `fixtures/eval/mycelia-v1-code.json` reported Mycelia 5/5 hits, baseline 4/5 hits, Mycelia tokens/answer 1219.8, baseline tokens/answer 29128.75; MCP smoke ok; stats ok.

## Decisions

- Phase 0 eval tasks use `required_files` as the file-level oracle. Legacy `expected` manifests remain supported for diagnostic fixtures, but v1 gate evidence should use `required_files`.
- `mycelia eval` rejects a manifest inside the indexed corpus. Fixture manifests under `fixtures/eval/` are still excluded from discovery, and measurement runs should copy/reference the manifest from outside the corpus under test.
- Paired Phase 0 reporting uses `mycelia eval --paired`. The baseline is named `grep_read`: it ranks live files under the same corpus root with deterministic lexical scoring and bills bytes read through the first required file.

## Blockers / questions for the lead

- (none)
