# Mycelia v1 — Evaluation & validation methodology

This is how every go/no-go gate in [../ROADMAP.md](../ROADMAP.md) is measured. These rules are hard-won; do not relax them to make a gate pass.

## The three-metric gate
Every retrieval change is judged on all three, together:
1. **Hit rate** — did the answer reach the file(s) a correct answer requires?
2. **MRR** — how highly ranked was the first correct result?
3. **Tokens per answered query** — total tokens consumed to reach the answer.

A hit-rate gain that increases tokens-per-answer is **not a win**. The point of the product is fewer tokens to the right context.

## Fixed task set with pre-declared expected files
- Each task names the specific file(s) a correct answer must reach.
- Success is measured by whether those files were reached — not by output prose.
- For PR-review tasks, the task also declares the expected true findings (for false-positive / recall scoring).
- The v1 eval manifest schema uses `required_files` for that contract:

```json
{
  "limit": 5,
  "cases": [
    {
      "name": "core default find entrypoint",
      "query": "pub fn find_with_strategy database query RetrievalStrategy",
      "required_files": ["crates/mycelia-core/src/lib.rs"]
    }
  ]
}
```

- Eval manifests must live outside the corpus under test when `mycelia eval` runs. Fixture manifests under `fixtures/eval/` are excluded from discovery, but a measurement run should copy or reference the manifest from outside the indexed root.

## Paired A/B, never single observations
- Compare Mycelia vs baseline (grep/read, or reviewer-alone) on the **same** prompt, repo, and time budget.
- One session with mandatory protocol reads is not a measurement. Run controlled pairs.
- Minimum **5 paired tasks** before any ship/no-ship decision.

## Transcript-visible tool calls only
- Only a record showing an **actual MCP tool invocation** counts as Mycelia usage.
- Claiming Mycelia use in prose while issuing shell `grep`/`cat` is a guidance failure, not a success — score it as a non-use.

## Token accounting
- Prefer harness-reported token counts.
- Fallback when unavailable: bytes of file/command output read before the first expected file is reached ÷ 4.
- Record `actual_tok` and `cold_tok` per `find` call from the activity log; `cold_tok` is the estimated cost of reaching the same context by cold read.

## The decision rule (ship gate)
Proceed/ship only if **both** hold across ≥ 5 paired tasks:
- **Token improvement ≥ 25%** vs baseline, **and**
- **No correctness regression** (no dropped true findings / no expected file missed).

For the Phase 4 PR-review bakeoff, add: **measurable false-positive improvement** over reviewer-alone.

## Standing guardrails
- **Measurement before storage rewrites.** Add the measurement first; brute-force vector similarity is acceptable until measured as the bottleneck.
- **Eval manifests are excluded from discovery** (R11) — oracle queries must never contaminate the corpus under test.
- **Freshness is correctness.** A stale slice that scores well is a failure, not a win.
