# Mycelia

> Code indexer for CI and agentic workflows, fully compatible with the Vercel AI SDK ecosystem.

Mycelia indexes a codebase with tree-sitter, materialises a deterministic index from a single commit, and serves precise, change-scoped code context to an agent you wrote — running headless in CI. It is the retrieval layer your agent calls, not the agent itself.

## What it is / what it is not

**It is:**

- A retrieval primitive: `find` (ranked headers) + `retrieve` (one fresh chunk)
- A read-only MCP server, consumable from any AI SDK 7.0 `createMCPClient` call
- A per-commit, deterministic, cache-friendly index, reproducible at a SHA
- CI-native: ephemeral builds, lexical-only path, no model download required

**It is not:**

- A PR reviewer, coding agent, or chatbot
- A desktop IDE assistant or persistent watcher-synced knowledge graph
- A document or prose indexer (code only)
- A replacement for the agent you write

## Usage

### 1. In CI (GitHub Actions)

```yaml
- uses: actions/checkout@v4
- uses: actions/cache@v4
  with:
    path: .mycelia/
    key: mycelia-${{ runner.os }}-${{ github.sha }}
    restore-keys: mycelia-${{ runner.os }}-
- run: mycelia ci prepare # build/restore index at this SHA, emit cache key + env
- run: node review-agent.mjs # your AI SDK agent queries the index
```

See [docs/vision.md](docs/vision.md) for the full CI narrative and rationale.

### 2. From a Vercel AI SDK 7.0 agent

```ts
import { ToolLoopAgent, stepCountIs } from "ai";
import { createMCPClient } from "@ai-sdk/mcp";

const mycelia = await createMCPClient({
  transport: { type: "stdio", command: "mycelia", args: ["serve"] },
});

const agent = new ToolLoopAgent({
  model: "anthropic/claude-sonnet-4-5", // via AI Gateway
  tools: await mycelia.tools(),
  stopWhen: stepCountIs(15),
});

const { text } = await agent.generate({
  prompt: "Review this PR using mycelia for context.",
});
```

See [docs/vision.md](docs/vision.md) for the full AI SDK narrative and the optional `@mycelia/ai-sdk` typed wrapper (Phase 3).

## Status today

**Working now:**

- Tree-sitter structural chunking for Rust, TypeScript, TSX, Python, Ruby; plain-text fallback for everything else
- Deterministic chunk IDs (BLAKE3) and extractor versioning, reproducible at any SHA (R3, R4)
- Freshness-validated retrieval: fresh chunk, live whole file on drift, or `unavailable` (R2)
- `mycelia ci prepare` for project-local CI indexes, R8 cache-key/env emission, and lexical-only CI by default
- Read-only MCP server (stdio) with six tools: `find`, `search_codebase`, `locate_implementation`, `retrieve`, `find_related`, `list_corpora` (R5)
- Rust `calls` graph: free-function, path, and macro call edges, with conservative query-time resolution
- Optional embeddings (BAAI/bge-small-en-v1.5 via FastEmbed/ONNX); lexical-only path works without them (R6)

**In progress per the roadmap:**

- Artifact export/import/verify with manifest (Phase 1, R7)
- Git-diff-aware incremental refresh (Phase 1)
- Change-scoped retrieval; TypeScript and Python `calls` graph (Phase 2)
- Reference `review-agent.mjs` + GitHub Actions workflow; optional `@mycelia/ai-sdk` wrapper (Phase 3)

See [ROADMAP.md](ROADMAP.md) for phases, gates, and sequencing.

## Build & test

```sh
# Install CLI to ~/.local/bin
cargo install --force --path crates/mycelia-cli --root "$HOME/.local"

# Full validation
cargo test --workspace --all-features
```

Per-slice protocol (before every merge):
`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test --workspace --all-features` → release build → CLI smoke → eval manifest run → MCP smoke → record stats. No broken tree between slices.

## Docs map

- [docs/vision.md](docs/vision.md) — what Mycelia is, the two usage narratives, the three differentiators, what is out of scope
- [docs/requirements.md](docs/requirements.md) — binding contract: R1-R11, engine constraints
- [docs/architecture.md](docs/architecture.md) — what exists today vs. the v1 target, gap list
- [docs/evaluation.md](docs/evaluation.md) — how every go/no-go gate is measured
- [ROADMAP.md](ROADMAP.md) — phase sequence, per-phase gates, definition of done
