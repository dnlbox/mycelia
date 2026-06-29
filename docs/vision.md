# Mycelia v1 — Vision

> Mycelia is a code indexer for CI and agentic workflows. Fully compatible with the Vercel AI SDK ecosystem.

It indexes a codebase with tree-sitter, materialises a **deterministic index from a single commit**, and serves precise, change-scoped code context to an **agent the developer wrote** — running headless in CI. It is not a coding assistant, not a chatbot, and not a PR reviewer. It is the retrieval layer those things call.

## The two things you do with Mycelia

**1. In your CI pipeline (GitHub Actions / CircleCI / Blacksmith):**
Build (or restore-then-incrementally-refresh) the index for the exact commit under review, then let your agent query it. The index build is a one-time, cached cost amortised across every query in the run.

```yaml
- uses: actions/checkout@v4
- uses: actions/cache@v4
  with:
    path: .mycelia/
    key: mycelia-${{ runner.os }}-${{ github.sha }}
    restore-keys: mycelia-${{ runner.os }}-
- run: mycelia ci prepare        # build/restore index at this SHA, emit cache key + env
- run: node review-agent.mjs     # your AI-SDK agent queries the index
```

**2. When building an agentic workflow that interacts with code (Vercel AI SDK 7.0):**
Mycelia is the MCP server (the portable spine) your agent connects to, or an optional typed npm wrapper. Your agent gets `find` (cheap ranked headers) and `retrieve` (one fresh chunk) instead of burning 30–50% of its context grepping.

```ts
import { ToolLoopAgent, stepCountIs } from 'ai';
import { createMCPClient } from '@ai-sdk/mcp';

const mycelia = await createMCPClient({
  transport: { type: 'stdio', command: 'mycelia', args: ['serve'] },
});
const agent = new ToolLoopAgent({
  model: 'anthropic/claude-sonnet-4-5',   // via AI Gateway
  tools: await mycelia.tools(),
  stopWhen: stepCountIs(15),
});
const { text } = await agent.generate({ prompt: 'Review this PR using mycelia for context.' });
```

## The three differentiators (the whole reason v1 exists)

The market converged on "persistent tree-sitter knowledge-graph MCP server for your assistant" (codebase-memory-mcp, Graphify, GitNexus, CodeGraph). Mycelia does **not** compete there. Its defensible intersection is unclaimed:

1. **Per-commit determinism — no drift.** The index is materialised fresh from one commit and is reproducible at that SHA. Everyone else ships a long-lived mutable graph patched by file watchers, so it cannot be trusted to reflect the exact code under review. Mycelia can.
2. **Library-shaped — consumed by a developer-written agent.** Built to be *called by code you wrote* with the AI SDK, not plugged into a desktop IDE assistant or rented as hosted memory.
3. **CI-native ephemerality.** Cached-incremental, keyed on tree hash; a Rust indexer makes warm-cache rebuilds seconds. Lexical-only path runs with no model download.

## The lead use case: be the lever, not the surface

PR review's binding constraint is **false-positive noise**, and its root cause is retrieval quality, not the model. Top reviewers cluster at F1 51–60%; trust collapses past ~20% noise. Every leader rebuilt the retrieval layer independently and none wants to own it — Greptile already proves the rental model exists.

So Mycelia ships the **retrieval primitive**, not another reviewer. The proof is a measurable bakeoff: *"reviewer-X + Mycelia beats reviewer-X alone on false-positive rate and token cost."* Land on the composable/open end (PR-Agent, Claude Code Action, home-grown CI agents); avoid head-to-head with closed end-to-end products.

## Explicitly out of scope for v1

These are abandoned, not deferred. Do not reintroduce them or their framing.

- Desktop-assistant / IDE adoption, per-harness `connect`, "consent boundary" framing.
- Being a PR reviewer, chatbot, or coding agent.
- Prose / PDF / document / docs indexing — **code only**.
- Persistent, drifting, watcher-synced graphs as the core product.
- Embeddings as the central bet (they remain an optional, measured strategy — see [requirements.md](requirements.md)).
- Multi-corpus / named-corpus profiles as a headline feature.

See [requirements.md](requirements.md) for the binding contract, [architecture.md](architecture.md) for what exists today vs. the target, [../ROADMAP.md](../ROADMAP.md) for the build sequence and go/no-go gates, and [evaluation.md](evaluation.md) for how every gate is measured.
