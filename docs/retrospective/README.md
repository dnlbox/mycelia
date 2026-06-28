# Mycelia retrospective

This retrospective is about the journey, not just the code.

Mycelia started as a local knowledge index, but the real project became a study
in how agents use context: prompts, harness behavior, MCP discovery, CLI product
design, Rust tradeoffs, measurement, and when to keep going versus pivot.

## Short version

Mycelia proved that a local index can find useful code context with fewer tokens
than opening whole files. It also proved something less comfortable: a good MCP
server does not matter if the harness or model keeps choosing native search
tools.

That changed the product thesis.

V1 asked:

```text
Can we build a precise local index?
```

V2 asks:

```text
Can a real agent, in a real harness, choose the index before it burns tokens?
```

That second question is the one that decides whether this becomes a product.

## Read order

1. [Timeline](01_timeline.md): what happened and why each turn mattered.
2. [Lessons](02_lessons.md): the reusable lessons from prompts, harnesses, MCP,
   CLI design, Rust, product pivots, and competitor checks.
3. [Evidence](03_evidence.md): local logs and repo artifacts used for this pass.

## The main lesson

Engineering quality is necessary, but adoption is the real system test.

The code can be fast, correct, and well packaged. The MCP server can be running.
The docs can say "use Mycelia first." None of that means an agent will use it.

The product has to meet the harness where it actually makes tool choices.

## What changed our mind

- Retrieval improved because we measured it, not because we guessed.
- The CLI got better when we focused on the beginner journey: `setup`,
  `connect`, `status`, `stats`, `refresh`, `list`, `delete`.
- Freshness became non-negotiable once stale chunks looked like a real trust
  problem.
- Competitor research did not kill the project. It clarified the wedge.
- The hardest problem moved from indexing to adoption.

## Current frame

The v2 frame is three planes:

- Index plane: project-owned `.mycelia/` config, database, logs, and cache.
- Guidance plane: harness-readable instructions that are visible, removable, and
  consent-gated.
- Connection plane: one machine-level `connect` action per harness install.

The publish-or-shelf gate is now practical: paired Codex and Claude Code runs
must show that Mycelia gets used organically and reaches the right files with
fewer orientation tokens than baseline grep/read behavior.
