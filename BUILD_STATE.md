# Mycelia v1 — build state

Working memory for the looping build agent. **Read first, update last, every slice.** The team lead reviews at each go/no-go gate; the build agent never crosses a gate on its own. See [prompt.md](prompt.md) for the loop, [ROADMAP.md](ROADMAP.md) for the phases.

## Position

- **Phase:** 0 — Measurement baseline & determinism proof
- **Slice:** not started
- **Status:** ready to begin
- **Tree:** green (engine inherited from `main`; see [docs/architecture.md](docs/architecture.md) for what exists today)

## Next up

Phase 0, first item: stand up the eval harness with a **fixed task set** where each task pre-declares the file(s) a correct answer requires. See [ROADMAP.md](ROADMAP.md) Phase 0 and [docs/evaluation.md](docs/evaluation.md).

## Gate status

- [ ] **GO/NO-GO 0** — determinism + measurement baseline (awaiting Phase 0 slices)
- [ ] GO/NO-GO 1 — per-commit index + CI artifact
- [ ] GO/NO-GO 2 — change-scoped retrieval
- [ ] GO/NO-GO 3 — Vercel AI SDK 7.0 integration
- [ ] GO/NO-GO 4 — SHIP

## Done log (append-only, terse — newest last)

- 2026-06-29 — v1 reset: docs + roadmap + build-loop established; engine inherited from `main` (tree-sitter chunking for Rust/TS/TSX/Python/Ruby, SQLite schema v5, deterministic chunk IDs, freshness, read-only MCP, Rust calls graph). Nothing built against the v1 roadmap yet.

## Decisions

- (none yet)

## Blockers / questions for the lead

- (none)
