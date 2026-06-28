# Product thesis

> Reconciled by `00_vision.md` (the canonical spine). Where this file and the
> vision differ, the vision wins.

## Why pivot

The v1 journey proved the engine and the MCP surface, but it still depends too
much on user-level harness configuration and agent memory. Prior-art review of
Graphify and `codebase-memory-mcp` shows the same adoption lesson: the index can
be strong and still lose to grep unless the agent is guided at the point of use.

The difference is where Mycelia should apply that guidance.

Graphify and `codebase-memory-mcp` lean into broad user-level integration:
global agent config, hooks, installed skills, and reminders. That can make
"install, restart, done" feel real, but it is invasive for teams. A company
running headless agents from Linear or GitHub issues usually wants repo-owned,
reviewable behavior, not hidden mutations under `~/.claude`, `~/.codex`, or
other user config trees.

V2 makes the project the integration boundary.

## Thesis

Mycelia is project-attached context infrastructure for AI implementation agents.

It gives a repo a local, source-verified, range-addressed map that can be used by:

- a developer's local harness,
- a headless CI agent producing PR proposals,
- a team-shared project workflow,
- a future harness that embeds `mycelia-core` directly.

The same project metadata and index lifecycle should serve all four.

## What changes from v1

V1 centered on named local corpus profiles:

```text
mycelia setup
mycelia connect codex
```

V2 centers on project self-discovery:

```text
mycelia init
mycelia serve
```

`init` creates or updates `.mycelia/` inside the project. `serve` walks up from
cwd, finds `.mycelia/config.toml`, and binds the current project without needing
a per-user registry entry for that project.

Global install is still useful, but it installs the binary and one generic MCP
server, not project-specific user config. Project behavior lives in the repo.

## Non-goals

- No silent writes to user-level harness config.
- No silent or default-yes mutation of project files outside `.mycelia/`.
  Consent-gated, convention-aware integration into existing instruction files
  (`AGENTS.md`, `CLAUDE.md`, Cursor rules) is the guidance plane's job; see
  `00_vision.md`. The default answer for any outside write is still no.
- No hard hooks that block grep, glob, read, or edit.
- No committed binary index by default.
- No abandonment of the v1 precision and freshness guarantees.

## Success definition

V2 succeeds when a team can answer yes to all of these:

- Can a fresh local agent discover the project index without reading a wiki?
- Can a headless CI agent restore or build the index before spending LLM tokens?
- Can a reviewer see every Mycelia-owned project file in git?
- Can the team decide whether indexes are local-only, cached in CI, published as
  artifacts, or explicitly committed?
- Can a harness adopt `mycelia-core` without inheriting CLI or MCP assumptions?

