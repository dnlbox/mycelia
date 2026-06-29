# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 0 — Measurement baseline & determinism proof
- **Slice:** Phase 0 / Slice 1 complete
- **Status:** ready for next Phase 0 slice
- **Tree:** green (2026-06-29: fmt, clippy, tests, release build, CLI smoke, eval run, MCP smoke, stats)

## Next up

Phase 0, second item: wire the three-metric report as a paired A/B run (Mycelia vs grep/read baseline). The current evaluator reports hit rate, MRR, and tokens-per-answered-query for Mycelia; the next slice should add the baseline side and paired output.

## Gate status

- [ ] **GO/NO-GO 0** — determinism + measurement baseline (awaiting Phase 0 slices)
- [ ] GO/NO-GO 1 — per-commit index + CI artifact
- [ ] GO/NO-GO 2 — change-scoped retrieval
- [ ] GO/NO-GO 3 — Vercel AI SDK 7.0 integration
- [ ] GO/NO-GO 4 — SHIP

## Done log (append-only, terse — newest last)

- 2026-06-29 — v1 reset: docs + roadmap + build-loop established; engine inherited from `main` (tree-sitter chunking for Rust/TS/TSX/Python/Ruby, SQLite schema v5, deterministic chunk IDs, freshness, read-only MCP, Rust calls graph). Nothing built against the v1 roadmap yet.
- 2026-06-29 — Phase 0 / Slice 1: added v1 fixed-task eval schema (`required_files`), emitted required files in eval results, rejected in-corpus eval manifests at runtime, added `fixtures/eval/mycelia-v1-code.json` with five code-only tasks, and documented the manifest contract. Validation: fmt ok; clippy ok; workspace tests ok; release build ok; install ok; CLI smoke ok; eval run 5/5 hits, MRR 0.8000, tokens/answer 1219.8; MCP smoke ok; stats ok.

## Decisions

- Phase 0 eval tasks use `required_files` as the file-level oracle. Legacy `expected` manifests remain supported for diagnostic fixtures, but v1 gate evidence should use `required_files`.
- `mycelia eval` rejects a manifest inside the indexed corpus. Fixture manifests under `fixtures/eval/` are still excluded from discovery, and measurement runs should copy/reference the manifest from outside the corpus under test.

## Blockers / questions for the lead

- (none)
