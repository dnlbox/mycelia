# CI and headless agents

## Why CI matters

Headless implementation agents are a better initial v2 wedge than local user
harnesses alone. A PR proposal agent starts with structured intent from Linear
or GitHub, then burns tokens discovering files, symbols, tests, and conventions.
Mycelia can do that discovery before the LLM starts broad reading.

The CI job already has:

- a clean checkout,
- a known commit,
- an issue or ticket body,
- a deterministic environment,
- a place to cache artifacts,
- a clear success/failure boundary.

This makes Mycelia's value easier to measure than in an interactive local
session.

## `mycelia ci prepare`

Purpose:

- locate `.mycelia/config.toml`,
- restore a cached or published index if available,
- validate schema, extractor versions, source commit, and model identity,
- refresh changed files,
- emit paths and status for the agent harness.

Sketch:

```text
mycelia ci prepare \
  --issue-file issue.md \
  --emit-env .mycelia/ci.env \
  --emit-summary .mycelia/ci-summary.json
```

Output should make the next step obvious:

```text
Mycelia project: my-project
Index: ready
DB: .mycelia/db/index.sqlite
Freshness: 42 files refreshed, 0 unavailable
Agent hint: start MCP with `mycelia serve --project /workspace/repo`
```

## `mycelia ci seed-context`

Purpose:

- turn ticket/issue text into a compact, sourced orientation pack,
- give the headless agent likely starting points,
- avoid loading whole files into the initial prompt.

Input:

- issue title and body,
- optional labels,
- optional changed files if this is a follow-up PR,
- optional team conventions from `.mycelia/AGENTS.md`.

Output:

```json
{
  "project": "my-project",
  "index_status": "ready",
  "likely_files": [],
  "likely_symbols": [],
  "recommended_queries": [],
  "tests_to_check": [],
  "warnings": []
}
```

This does not replace MCP. It seeds the first agent turn so the agent starts with
better retrieval calls.

## PR proposal flow

```text
checkout repo
install mycelia
restore cache
mycelia ci prepare
mycelia ci seed-context --issue-file issue.md --json > context.json
start headless agent with:
  - project instructions
  - context.json
  - Mycelia MCP command
agent writes patch
run validation
upload stats and PR
save Mycelia cache
```

## Merge and main-branch artifact flow

Avoid committing DB updates on every PR by default.

Preferred:

- PR CI builds or refreshes the index for validation and agent use.
- Main-branch CI publishes an index artifact keyed by commit SHA.
- Future PR jobs restore the closest cache or main artifact, then incrementally
  refresh.

Artifact naming should include:

- project name,
- git commit,
- schema version,
- extractor versions,
- embedding model and dimensions,
- Mycelia version.

## Security and consent

CI mode must not:

- write user-level harness config,
- push index artifacts to a public store without explicit configuration,
- include ignored or secret files unless the project config says so,
- serve stale slices when sources changed.

CI mode should:

- respect gitignore by default,
- print the exact artifact/cache paths,
- emit a machine-readable summary,
- fail clearly when the index cannot be trusted.

## Measurements

A CI adoption slice should measure:

- index restore time,
- refresh time,
- index size,
- query latency,
- tokens in seed context,
- tokens spent by the agent before first patch,
- whether the agent touched the expected files,
- validation pass/fail,
- total PR runtime.

The business claim is not only speed. It is lower token spend and better first
implementation proposals from issue text.

