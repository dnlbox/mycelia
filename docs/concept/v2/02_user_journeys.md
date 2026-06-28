# User journeys

## Journey A: local developer, project already initialized

Prerequisites:

- Mycelia binary installed on the machine.
- Project contains `.mycelia/config.toml`.
- The local harness can launch one generic Mycelia MCP server, or the user starts
  `mycelia serve` manually.

Flow:

```text
git clone <repo>
cd <repo>
mycelia status
mycelia serve
```

Expected behavior:

- `status` finds `.mycelia/config.toml` by walking up from cwd.
- If `.mycelia/db/index.sqlite` is missing, `status` names the fix:
  `run mycelia refresh` or import a team artifact.
- `serve` exposes one MCP surface and resolves the project from cwd.
- The model sees instructions from `.mycelia/AGENTS.md` when the harness includes
  or imports that file.

What the user should not have to do:

- Register a named corpus in a global profile.
- Edit `~/.codex/config.toml` or `~/.claude/settings.json` per project.
- Remember a project-specific database path.

## Journey B: developer initializes a project for the team

Prerequisites:

- Mycelia binary installed.
- User is at the project root, or inside a git worktree.

Flow:

```text
mycelia init
```

`init` creates:

```text
.mycelia/
  config.toml
  AGENTS.md
  db/              # ignored by default
  logs/            # ignored by default
  artifacts/       # ignored by default unless team opts in
```

Then it proposes any optional integration outside `.mycelia/` as a preview:

```text
Detected AGENTS.md at project root.
Suggested change:
  add a one-line reference to .mycelia/AGENTS.md

Apply this change? [y/N]
```

If declined, the project is still initialized. Mycelia never needs root
instruction mutation to function; it only improves harness adoption.

Team decision:

- Commit `.mycelia/config.toml`.
- Usually commit `.mycelia/AGENTS.md`.
- Usually ignore `.mycelia/db/`, `.mycelia/logs/`, and `.mycelia/artifacts/`.
- Optionally commit index artifacts or live DB only after an explicit team
  decision.

## Journey C: team project with shared CI cache

Prerequisites:

- Project committed `.mycelia/config.toml`.
- CI can install or cache the Mycelia binary.
- CI has a cache or artifact store.

Flow:

```text
mycelia ci prepare
```

Expected behavior:

- Restore an index artifact or cache keyed by project config, extractor versions,
  model identity, and source commit.
- Incrementally refresh changed files.
- Emit environment metadata for the headless agent:

```text
MYCELIA_PROJECT_ROOT=/workspace/repo
MYCELIA_DB=/workspace/repo/.mycelia/db/index.sqlite
MYCELIA_MCP_COMMAND=mycelia serve --project /workspace/repo
```

The headless agent can now use Mycelia before reading broad file sets.

## Journey D: headless Linear or GitHub issue agent

Prerequisites:

- The CI job has a ticket or issue body.
- `mycelia ci prepare` has produced a usable index.
- The agent harness can call MCP, CLI JSON, or an embedded adapter.

Flow:

```text
mycelia ci prepare
mycelia ci seed-context --issue-file issue.md --json > mycelia-context.json
start-headless-agent --context mycelia-context.json
```

The seed context should include compact, sourced hints:

- likely files,
- likely symbols,
- tests near the touched areas,
- relevant docs,
- retrieval commands to call next,
- warnings if the project is unindexed or stale.

The agent still owns the patch. Mycelia only reduces orientation tokens and
raises the chance that the first implementation plan names the right files.

## Journey E: harness embeds Mycelia as a library

Prerequisites:

- Harness links or shells to `mycelia-core`.
- Project has `.mycelia/config.toml`.

Flow:

- Harness asks Mycelia to resolve project config from cwd.
- Harness calls `find`, `retrieve`, and graph operations through a stable API.
- Harness uses the same freshness and token-budget semantics as MCP.

This path is the clean long-term adoption story. The CLI and MCP remain useful,
but the core contract must not depend on either.

