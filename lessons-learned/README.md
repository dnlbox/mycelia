# Mycelia: lessons learned

Mycelia is shelved as of 2026-06-30. This is the honest record of why, written so
the next idea starts from the evidence instead of relearning it the expensive way.
A failed experiment that is clearly recorded is worth as much as a feature.

## What we were trying to build

A code indexer that gives an AI agent two tools, `find` (cheap ranked headers) and
`retrieve` (one exact chunk), so the agent reaches the code it needs without
re-reading whole files. Over the project the framing sharpened three times:

1. **v0:** a local, content-agnostic knowledge index (codebases, notes, docs).
2. **v2:** project-attached infrastructure adopted through coding harnesses
   (Claude Code, Codex, Cursor, Antigravity). This stalled on adoption: we could
   not reliably get harnesses to actually call the tools.
3. **v1 (the final, sharpest framing):** drop harness breadth; target the
   **AI-SDK-in-CI** seam. The customer is an *already-built* agent (someone's
   Vercel AI SDK / Claude / ADK PR-review, code-review, or issue-to-PR agent) that
   today navigates with only grep and read. The promise was narrow and honest:
   **save tokens, not find bugs.** Same job, fewer tokens.

## How we tested it

A gated build: five phases (measurement baseline, per-commit index + CI artifact,
change-scoped retrieval, Vercel AI SDK 7.0 integration, and a final bakeoff), each
with a go/no-go gate that a reviewer had to clear by reproducing the evidence, not
trusting the builder's report. The discipline caught real problems at every gate
(a metric inflated by a result-limit, a missing `/v1` in a provider URL, an agent
that over-explored). It worked.

## What we found (the refutation)

The final ship gate was a bakeoff: the **same** model-backed agent, **with** vs
**without** Mycelia, on real tasks against `github.com/earlgreylabs/candelabrum-studio`.

- **Bug-finding (never our job, but checked):** 0 of 6 cross-file bugs found, by
  both arms, across three agent configurations. Cheap retrieval made the agent
  shallower; loose budgets made it loop without concluding.
- **Token efficiency (our actual job):** the agent **with** Mycelia used about
  **2.4x MORE tokens** than the grep/read agent for code navigation (-142% with
  the full tool surface, -138% even when stripped to a lean `find` + `retrieve`),
  and lost on every task.

A precomputed index did not beat agentic grep on a small or medium repository.

## Why it failed (structural, not tunable)

- `find` returns verbose ranked headers (path, range, signature, score, chunk id);
  grep returns terse `path:line:match`. `retrieve` returns a whole chunk; a
  targeted `read_file` returns only what is needed.
- Lexical ranking was mediocre, so the model re-searched (`find` called many times)
  instead of converging.
- The model is fluent with grep and read and flailed with `find` and `retrieve`.
- The bugs we first chose were "absence" bugs (a parameter not passed, a status
  never set), which are very hard to spot by reviewing what is present, and are not
  what a token-efficiency tool is for anyway.

## The biggest lesson: measure against a real agent, not a straw man

Early in the project we reported "90 percent plus token savings." Those came from
`mycelia eval --paired`, whose baseline was a **deterministic `grep_read`
simulation that reads whole files cold and bills every byte until it hits the
answer**. No real agent does that. And it measured Mycelia at its *ideal*: one
`find` plus one `retrieve`, about 1,200 tokens.

When a **real** agent finally used the tool, both sides changed:

- The real baseline (a competent grep/read agent) is 10x to 20x cheaper than the
  cold-read simulation.
- Real Mycelia is far more expensive than the ideal: the agent loops
  (`find, find, retrieve, ...`), and every tool result accumulates in the
  conversation, so cost grows roughly with the square of the number of steps. Real
  Mycelia cost was 80k to 120k tokens per task, not 1,200.

Both numbers were arithmetically correct; they described different worlds. The gap
we were selling lived between "naive cold read" and "competent grep," and was never
ours to claim.

**Rule for next time:** a retrieval or indexing tool must be measured against the
*actual alternative a competent agent would use* (grep/rg plus targeted reads), run
**end to end with full token accounting** (looping and context growth included),
not as an isolated retrieval micro-benchmark and not against a worst-case
simulation. Apply the same scrutiny to other tools' "99 percent fewer tokens"
claims: ask what baseline produced the number.

## What survives

- The Rust engine is solid: deterministic tree-sitter chunking (Rust, TS, TSX,
  Python, Ruby), content-hash freshness, SQLite persistence, a read-only MCP
  surface, a depth-1 Rust call graph, and a per-commit CI artifact path. The
  failure was the positioning, not the code.
- The gated build-and-review process, which found the truth at the bench instead
  of in a shipped, public, and wrong claim.

## What we would do differently

- Validate the value proposition with one real-agent A/B **before** building five
  phases of infrastructure around it.
- Treat the foundational competitive research as a falsifiable prior, not a
  background note: it warned that agentic grep is hard to beat, and it was right.
- If revisited, aim only at the narrow niche an index plausibly wins and test it
  directly: very large monorepos where grep itself is slow or expensive, and
  precise call-graph or blast-radius queries that grep cannot answer at all.

## Pointers

- Shelved implementation and full gate-by-gate record: the `v1` branch
  (`BUILD_STATE.md`).
- v0/v2 retrospective: `docs/retrospective/`.
