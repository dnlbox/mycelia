# Mycelia v2 concept pack

V2 reframes Mycelia from a user-machine MCP installer into project-attached
context infrastructure for agents.

The pivot:

- A project owns its Mycelia integration under `.mycelia/`.
- A single Mycelia MCP server discovers the current project from cwd and
  `.mycelia/config.toml`.
- Local harnesses, headless CI agents, and embedded harness/library integrations
  all consume the same project index contract.
- User-level harness config is never changed by default.
- Any write outside `<project>/.mycelia/` is shown first and requires explicit
  confirmation.

This folder is a rewrite layer, not a deletion of v1. It preserves the current
precision-first retrieval, freshness, MCP, graph, evaluation, observability, and
distribution requirements while changing the adoption surface.

## Documents

- `00_vision.md`: the canonical spine. Three planes, one consent boundary, and
  the connect/init lifecycle. Read this first; it reconciles the rest.
- `01_product_thesis.md`: why the pivot exists and what changes from v1.
- `02_user_journeys.md`: local, team, CI, and library adoption journeys.
- `03_project_layout_and_consent.md`: `.mycelia/` layout and write boundaries.
- `04_ci_and_headless_agents.md`: ticket-to-PR agent workflow and CI cache flow.
- `05_indexer_speed_and_artifacts.md`: speed, incremental indexing, and artifact
  strategy inspired by prior-art review.
- `06_visibility_and_diagnostics.md`: how users and teams see whether it works.
- `07_requirements_carry_forward.md`: v1 requirements that must survive v2.

## V2 north star

```text
git clone
mycelia init
git add .mycelia/config.toml .mycelia/AGENTS.md

# Local harness or CI:
mycelia serve
```

The first real product promise is not "we configured your machine." It is:

```text
This repository carries enough Mycelia metadata for any compatible agent,
local or headless, to discover and use the right project index.
```

