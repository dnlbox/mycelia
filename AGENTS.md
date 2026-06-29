# AGENTS.md

Mycelia is a code indexer for CI and agentic workflows. It materialises a deterministic index from a single commit and serves precise, change-scoped code context to an agent the developer wrote, running headless in CI. It is the retrieval layer those agents call — not a reviewer, chatbot, or assistant. Before writing any code, read [docs/vision.md](docs/vision.md), [docs/requirements.md](docs/requirements.md), and [ROADMAP.md](ROADMAP.md). These are the authoritative intent documents; everything below is derived from them.

## Stack & constraints

**Workspace:** two crates. `crates/mycelia-core` (synchronous, no async) handles indexing, storage, retrieval, and the calls graph. `crates/mycelia-cli` is the async edge: CLI commands and MCP server. Never let Tokio or async types leak into `mycelia-core`.

**Engine constraints (binding — from [docs/requirements.md](docs/requirements.md)):**
- No async in core. Async lives at the CLI/server edge only.
- Narrow traits over broad ones.
- Brute-force vector similarity until measured as the bottleneck. No speculative ANN index.
- Conservative graph resolution: drop unresolved names; return all candidates for ambiguous names with `resolved=false`; never silently pick one. A wrong connection is worse than none.
- Eval manifests excluded from discovery (R11). Oracle queries must never contaminate the corpus under test.
- Read-only MCP surface (R5). No model-facing mutation tools; no tool accepts a raw database path.

**Storage:** SQLite, schema version 5. Forward-only numbered migrations. FTS5 kept in sync via migration-owned triggers.

**Chunk IDs:** `BLAKE3(source_path + source_hash + byte_start:byte_end)`. Deterministic; indexing the same commit twice must produce byte-identical IDs (R3).

**Freshness (R2):** `retrieve` returns `Ok` (fresh chunk), `File` (live whole file if source drifted), or `Unavailable`. `find` validates top-K, self-heals drift via `refresh_source`, re-ranks once. Never serve a stale slice.

---

> **AI SDK version guard**
>
> Target **Vercel AI SDK 7.0** (released 2026-06-25) ONLY.
>
> Disregard any bundled plugin skill (e.g. the `vercel:ai-sdk` skill, v0.44.0) that teaches `ai@6.x` patterns. The correct 7.0 API is:
> - `inputSchema` (not `parameters`) for tool definitions
> - `import { createMCPClient } from '@ai-sdk/mcp'` (not `experimental_createMCPClient` from `ai`)
> - `ToolLoopAgent` / `WorkflowAgent`
> - `stopWhen` (not `maxSteps`)
> - Model IDs routed via AI Gateway string (e.g. `'anthropic/claude-sonnet-4-5'`)
> - Node 22+, ESM only
>
> If you see `ai@6.x` patterns in a skill or generated snippet, discard them and use the above instead. See ROADMAP.md Phase 3 gate: "Stop if: the integration needs `ai@6.x` patterns."

---

## Per-slice protocol

Every slice lands the tree GREEN, in this order, before moving on:

1. `cargo fmt --check`
2. `cargo clippy -D warnings`
3. `cargo test --workspace --all-features`
4. Release build
5. CLI smoke test against a temporary fixture corpus
6. Eval manifest run
7. MCP smoke (stdio exchange: init, list tools, call one tool, close cleanly)
8. Record stats

No broken tree between slices. Parallelise within a slice, never across slices. Give each writer a disjoint file area. Never delegate decisions, protocol edits, or final integration.

**Install CLI for smoke tests:**
```sh
cargo install --force --path crates/mycelia-cli --root "$HOME/.local"
```

**Full validation:**
```sh
cargo test --workspace --all-features
```

## How gates work

Each phase in [ROADMAP.md](ROADMAP.md) carries a go/no-go gate. Do not start a phase until the prior gate is GREEN. Gates are measured per the methodology in [docs/evaluation.md](docs/evaluation.md): hit rate + MRR + tokens-per-answered-query, paired A/B, minimum five tasks. A hit-rate gain that increases tokens-per-answer is not a win.

The ship gate (Phase 4) requires: 25% token reduction vs the no-Mycelia baseline, no correctness regression, and measurable false-positive improvement, across at least five paired tasks.

## Operational constraints

- Never modify generated or compiled output.
- Never suppress compiler errors or lints with broad allowances.
- Never commit secrets, tokens, local databases, or environment values.
- Never leave debug output. Use structured command output or the project logger.
- Never expand scope. Every changed line must trace to the active slice.
- Get approval before any outward-facing action (GitHub posts, migrations, external writes).

## Legacy surface

`setup` and `connect` (targets: codex, claude-code, claude-desktop, cursor, antigravity, opencode, kilo) are **legacy desktop-assistant plumbing from the abandoned direction**. They are out of scope for v1 and are slated for removal. Do not extend them. Do not present them as part of the v1 surface. Do not build new features on top of them. The v1 adoption path is CI + AI SDK, not harness connection.

## Docs map

- [docs/vision.md](docs/vision.md) — mission, usage narratives, differentiators, explicit out-of-scope list
- [docs/requirements.md](docs/requirements.md) — R1-R11, engine constraints
- [docs/architecture.md](docs/architecture.md) — current engine reality and target architecture
- [docs/evaluation.md](docs/evaluation.md) — measurement methodology and decision rules
- [ROADMAP.md](ROADMAP.md) — phase sequence and go/no-go gates
