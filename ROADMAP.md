# Mycelia v1 — Roadmap

End-to-end path from the current engine to a shipped v1, with a **go/no-go gate** on every phase. Do not start a phase until the prior phase's gate is GREEN. Read [docs/vision.md](docs/vision.md) and [docs/requirements.md](docs/requirements.md) first; every gate is measured per [docs/evaluation.md](docs/evaluation.md).

## Definition of done for v1

A developer can add ~10 lines to a GitHub Actions workflow and have a headless Vercel-AI-SDK agent review a PR using Mycelia for change-scoped context, and we can show — on a public, paired benchmark — that the agent **with** Mycelia beats the same agent **without** it on token cost and false-positive rate.

## Per-slice protocol (applies inside every phase)

Every slice lands the tree GREEN, in this order, before moving on:
`cargo fmt --check` → `cargo clippy -D warnings` → `cargo test --workspace --all-features` → release build → CLI smoke → eval manifest run → MCP smoke → record stats. No broken tree between slices.

---

## Phase 0 — Measurement baseline & determinism proof

_Foundation. Nothing else is trustworthy without this._

- [x] Stand up the eval harness: a **fixed task set** where each task declares the file(s) a correct answer requires (per [docs/evaluation.md](docs/evaluation.md)).
- [x] Wire the three-metric report: hit-rate + MRR + tokens-per-answered-query, paired A/B (Mycelia vs grep/read baseline).
- [x] Verify the lexical-only path (R6) and deterministic IDs (R3).

**GO/NO-GO 0:**

- [ ] Index built twice at the same SHA is byte-identical (R3).
- [ ] Eval harness emits all three metrics on paired runs, reproducibly.
- [ ] `find` works with `--no-embed`, zero network (R6).
- **Stop if:** IDs are not reproducible at a SHA — determinism is differentiator #1; fix before proceeding.

---

## Phase 1 — Per-commit index + CI artifact

_Differentiators #1 and #3 become real._

- [x] Implement `mycelia ci prepare`: build-or-restore the index at the current commit, validate schema, emit the cache key (R8) and env for downstream steps.
- [x] Implement artifact `export` / `import` / `verify` with the manifest format (R7).
- [x] Implement git-diff-aware **incremental refresh** (restore previous artifact → update only changed files).

**GO/NO-GO 1:**

- [ ] `export` → `import` → `verify` round-trips; mismatched manifest field is rejected with a named reason (R7).
- [ ] Cache key changes iff a composing input changes; unchanged tree reuses artifact byte-for-byte (R8).
- [ ] Cold build (medium repo) within CI setup budget; warm incremental refresh in seconds (R10).
- **Stop if:** warm refresh is not materially faster than cold build — the CI cost story collapses.

---

## Phase 2 — Change-scoped retrieval (the PR-review wedge)

_Turn the diff into the query._

- Given a diff, return the **blast radius**: changed symbols plus their callers/callees via the `calls` graph.
- Extend symbol + `calls` extraction to **TypeScript** (Vercel-ecosystem priority; Python next). Keep conservative resolution (engine constraints in [docs/requirements.md](docs/requirements.md)).
- Expose change-scoped retrieval through `find_related` / `find`.

**GO/NO-GO 2:**

- [ ] On a labelled PR set, change-scoped retrieval surfaces the cross-file files a correct review needs (first-file hit rate ≥ baseline grep).
- [ ] TS call edges resolve on a real TS repo; ambiguous names return `resolved=false`, never a silent guess.
- [ ] Tokens-per-answer for "what does this change affect" beats the grep/read baseline.
- **Stop if:** change-scoped retrieval does not beat plain `find` on the PR task set — the wedge isn't there yet.

---

## Phase 3 — Vercel AI SDK 7.0 integration

_Differentiator #2: consumed by a developer-written agent._

- Verify the MCP server is consumable via `@ai-sdk/mcp` `createMCPClient` (stdio for CI, HTTP option).
- Ship a reference `review-agent.mjs`: `ToolLoopAgent` + AI Gateway model routing, Node 22 ESM, runs headless.
- Ship the reference GitHub Actions workflow (checkout → cache → `mycelia ci prepare` → agent).
- _Optional:_ thin typed `@mycelia/ai-sdk` wrapper exporting `tool()` defs (only after the MCP surface is re-audited for deterministic, structured output).

**GO/NO-GO 3:**

- [ ] A headless AI-SDK agent in a real GitHub Action queries Mycelia and produces a PR review comment.
- [ ] Works on Node 22+ ESM with AI SDK 7.0 (`inputSchema`, `createMCPClient` from `@ai-sdk/mcp`, `stopWhen`).
- [ ] End-to-end run stays within a sane token/time budget on a medium repo.
- **Stop if:** the integration needs `ai@6.x` patterns — re-pin to 7.0 (see [AGENTS.md](AGENTS.md)).

---

## Phase 4 — The proof (ship gate)

_The bakeoff that makes the positioning credible instead of asserted._

- Build a public paired benchmark: a reviewer (PR-Agent or Claude Code Action) **+ Mycelia** vs the **same reviewer alone**, across ≥ N real PRs with labelled expected findings.
- Report token cost, false-positive rate, and first-file hit rate.

**GO/NO-GO 4 (SHIP):**

- [ ] ≥ 25% token reduction vs the no-Mycelia baseline.
- [ ] No correctness regression (no dropped true findings).
- [ ] Measurable false-positive improvement.
- [ ] Both conditions hold across **≥ 5 paired tasks** (the decision rule).
- **If GREEN:** cut v1, publish the benchmark and the two integration recipes. **If not:** the failing metric names the next slice; loop back.

---

## Sequencing notes

- Phases are strictly ordered: 0 → 1 → 2 → 3 → 4. Each gate guards the next.
- Embeddings stay optional and measured throughout; do not make them the critical path (R6, engine constraints).
- Anything not on this roadmap is out of scope for v1 (see [docs/vision.md](docs/vision.md) "Explicitly out of scope").
