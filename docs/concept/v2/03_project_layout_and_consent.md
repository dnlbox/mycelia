# Project layout and consent

> Reconciled by `00_vision.md`. `.mycelia/AGENTS.md` is the source fragment that
> `init` wires into the project's existing conventions, not the only guidance
> file. The write-boundary and consent rules below stand unchanged.

## Default project layout

V2 writes inside `.mycelia/` by default:

```text
.mycelia/
  config.toml
  AGENTS.md
  db/
    index.sqlite
  logs/
    activity.log
  artifacts/
    index.tar.zst
  cache/
```

Committed by default:

- `.mycelia/config.toml`
- `.mycelia/AGENTS.md`

Ignored by default:

- `.mycelia/db/`
- `.mycelia/logs/`
- `.mycelia/cache/`
- `.mycelia/artifacts/`, unless the team explicitly chooses artifact-in-git.

## `.mycelia/config.toml`

Purpose:

- declare the project root identity,
- declare discovery/include/exclude policy,
- record extractor and schema expectations,
- declare whether embeddings are enabled,
- declare CI artifact/cache policy,
- avoid storing arbitrary absolute user-machine paths.

Sketch:

```toml
[project]
name = "my-project"
root = "."

[index]
database = ".mycelia/db/index.sqlite"
logs = ".mycelia/logs/activity.log"
mode = "local"

[discovery]
respect_gitignore = true
exclude = ["fixtures/eval/*.json"]

[ci]
cache = true
artifact = "optional"

[instructions]
file = ".mycelia/AGENTS.md"
```

## `.mycelia/AGENTS.md`

This file carries Mycelia-specific project guidance created during `mycelia init`.

It should be short, stable, and safe to commit. It does not replace root
`AGENTS.md` or `CLAUDE.md`; it is a Mycelia-owned fragment that other
instructions can include or reference.

Minimum content:

```md
# Mycelia Project Context

This project has a Mycelia index in `.mycelia/`.

For code orientation:

1. Use Mycelia `find` first for broad implementation, symbol, or docs discovery.
2. Use `search_codebase` or `locate_implementation` when those names better
   match the user task; they are aliases for `find`.
3. Use `retrieve` for selected chunks.
4. Use `find_related(symbol, direction)` for callers/callees when relationships
   matter.
5. Use `list_corpora` only when another project is named ambiguously.
6. Use grep/read for exact line edits, literal strings, generated files, and
   fallback after Mycelia misses.

Few-shot patterns:

- "Where is X implemented?" -> `locate_implementation("X implementation")`,
  then `retrieve` the best chunk.
- "What calls X?" -> `find_related("X", direction="callers")`.
- "Orient on the current slice" -> `find` with slice, roadmap, state, and
  concept keywords, then retrieve the state and roadmap chunks.

The current project corpus is discovered from `.mycelia/config.toml`.
```

## Guidance plane: convention detection

`mycelia init` does not only drop `.mycelia/AGENTS.md`. It detects whatever
instruction convention the project already uses and offers to wire a
Mycelia-owned block into it, so the "use Mycelia first" guidance reaches the
model through the path that harness already reads.

Conventions to detect:

- `AGENTS.md` and nested `AGENTS.md`
- `CLAUDE.md`
- Cursor rules (`.cursor/rules/*.mdc`)
- Codex project configuration
- Antigravity project rules
- OpenCode project configuration and `AGENTS.md`
- Kilo project rules
- Claude Code project settings (`.claude/settings.json`), including eager tool
  loading so a deferred MCP schema does not lose to grep

Every such write is outside `.mycelia/`, so it follows the write boundary below:
previewed, confirmed, idempotent, and removable. Declining still leaves a working
project; guidance only improves adoption.

## One MCP

There should be one generic Mycelia MCP server:

```text
mycelia serve
```

The server resolves the current project by walking up from cwd to
`.mycelia/config.toml`. It should not require one MCP server entry per project.

Resolution order:

1. explicit `--project <path>`,
2. cwd walk-up to `.mycelia/config.toml`,
3. optional legacy named corpus fallback,
4. explicit error with `run mycelia init`.

## Write boundary

Default allowed writes:

- create/update files under `<project>/.mycelia/`,
- add recommended `.gitignore` entries only after confirmation,
- update the project index under `.mycelia/db/`.

Any write outside `<project>/.mycelia/` must be previewed and confirmed.

Examples that require preview and confirmation:

- adding an include line to root `AGENTS.md`,
- adding a Mycelia note to `CLAUDE.md`,
- adding harness-specific project rules,
- editing `.gitignore`,
- writing CI workflow snippets,
- writing user-level config under `~/.codex`, `~/.claude`, `~/Library`, or
  equivalent paths.

Default answer for outside writes is no.

## Committing the index

The live SQLite DB may live under `.mycelia/db/`, but it is not committed by
default.

Reasons:

- noisy binary diffs,
- merge conflicts,
- possible stale source snippets,
- platform and feature-version churn,
- poor review ergonomics.

Supported modes:

| Mode | Default | Git policy | Use case |
| --- | --- | --- | --- |
| local DB | yes | ignored | local harness use |
| CI cache | yes for CI | external cache | PR proposal agents |
| artifact export | optional | external artifact store | team sharing |
| committed DB | explicit opt-in | committed | small repos, frozen docs, demos |

Opt-in command sketch:

```text
mycelia init --commit-index
```

It must print a warning, explain binary conflict risk, and confirm before
changing ignore rules.
