# Dogfood and protocol adoption

> Updated by `docs/concept/v2/00_vision.md`. The guidance plane (convention
> detection and consent-gated integration into existing instruction files) is now
> first-class, not the opt-in afterthought framed below. The no-silent-mutation
> rule and the `stats`-as-adoption-surface direction still hold.

Planned next slice. This spec turns the retrospective into product work: Mycelia
must be the path agents actually use, not merely a healthy MCP server they could
use.

## Problem

The installed MCP is not enough. In normal sessions, Codex and Claude still often
choose shell search, `Read`, `Grep`, or an Explore agent before Mycelia. This is
not a retrieval-engine failure. It is an adoption and protocol failure:

- Mycelia is available but not always visible in the agent's immediate tool set.
- Always-visible shell tools have stronger model habit and simpler affordance.
- Project protocols say what files to read, but not when to use Mycelia as the
  orientation path.
- `stats` proves value only after calls happen; it does not yet make non-use
  obvious enough.

The goal is not to force tool use. The goal is to make the efficient path the
obvious path, and to make failure to use it measurable.

## Product stance

Mycelia should win by judgment, not by interception. Hard hooks that block grep
or silently rewrite commands are out of scope. Soft, transparent guidance is in
scope when the user can see, approve, and remove it.

The desired pattern is:

1. Read mandatory protocol files directly (`AGENTS.md`, `prompt.md`,
   `BUILD_STATE.md`) because they define the contract.
2. Use Mycelia `find` / `locate_implementation` / `search_codebase` for broad
   orientation, implementation hunts, feature support questions, and
   cross-corpus lookups.
3. Use `retrieve` for the selected chunks.
4. Use shell grep/read for exact line edits, validation, and fallback after
   Mycelia narrows the search or misses.

## Scope for slice 24

### 1. Adoption observability through `stats`

Do not add a separate `doctor adoption` command yet. `stats` is the value and
adoption surface.

Add:

- `mycelia stats --all`: show every registered corpus with query count, recent
  last-use time, actual tokens, cold-read estimate, and savings ratio.
- Clear zero-use language: a registered corpus with no `find` calls should say
  that no agent has used Mycelia for it yet, not merely that no stats exist.
- Recent activity by default or via an obvious flag: keep `--recent N`, but make
  the default `stats` output point users at it when diagnosing adoption.
- A small "next check" hint: when there are no calls after `connect`, suggest
  running a task and then `mycelia stats --recent 20` to verify whether the
  harness used Mycelia.

Acceptance:

- `stats --all` reads logs only, no DB access.
- Empty, missing, and active logs are all handled without error.
- Output stays compact enough to paste into a support/debug thread.

### 2. Transparent harness guidance

`connect` may install guidance in harness-owned configuration where that is the
normal integration path. It must not silently mutate arbitrary project source
files.

Acceptable first cuts:

- For Codex config, include server metadata or generated instructions only if the
  target config format supports it. If the config only supports command/args,
  leave it alone.
- For Claude Code, prefer the official MCP config path and project-local guidance
  only when the user explicitly opts in.
- For Claude Desktop, rely on MCP instructions and improve `stats` diagnostics;
  Desktop has no reliable project instruction file.

Optional opt-in:

```text
mycelia connect <harness> --install-guidance
```

If implemented, it writes an obvious, idempotent Mycelia-owned block, for
example under `.agents/` where the project already uses that convention. It must
print every path it will write before doing so and explain how to remove it.

Avoid:

- Hidden `.agents/mycelia` creation during `setup`.
- Modifying `AGENTS.md` or `prompt.md` without explicit user approval.
- Any hook that blocks grep/read.

Acceptance:

- Default `setup` and `connect` remain non-invasive.
- Opt-in guidance can be smoke-tested in an isolated temp project.
- Re-running the command updates one owned block, never duplicates it.

### 3. Soft hook research, not default behavior

A Claude soft hook may be useful, but it is not the first implementation target.
Graphify's experience shows hooks can nudge behavior, but they are fragile across
harnesses and can feel coercive.

If explored, the hook should only remind before broad search/read operations in
a registered corpus:

```text
Mycelia is registered for this project. Use `find` first for broad orientation
unless this is an exact one-line lookup or Mycelia already missed.
```

Acceptance before promotion:

- It is opt-in.
- It never blocks the original command.
- It can be disabled cleanly.
- It measurably increases Mycelia calls in paired sessions without increasing
  task time or user irritation.

### 4. CLI surface cleanup

The command list is growing and the retired `corpus` group now adds noise.

Change:

- Remove the visible retired `corpus` command group from top-level help.
- Keep a helpful error for `mycelia corpus ...` only if Clap supports hidden
  aliases/subcommands cleanly; otherwise prefer removing it and letting the
  normal unknown-command help point users at the documented journey.
- Keep user journey help focused on:
  `setup`, `connect`, `stats`, `status`, `refresh`, `list`, `delete`.
- Keep power/diagnostic commands available:
  `find`, `retrieve`, `graph`, `eval`, `embed`, `index`, `serve`.

Rationale:

- `find`, `retrieve`, and `graph` stay in the CLI because they are required for
  diagnostics, tests, and human inspection of MCP behavior.
- `serve` stays available because harnesses launch it, but it should remain
  framed as internal plumbing.
- `eval`, `embed`, and `index` stay because they are the measured development
  surface and fixture path.

Acceptance:

- `mycelia --help` presents the journey first and does not advertise retired
  `corpus`.
- Existing tests that intentionally check retired-command migration are updated
  or removed according to the final Clap shape.
- README and Project Specifics match the visible command groups.

### 5. Dogfood gate

Every Mycelia slice closeout should record whether Mycelia was used for
orientation.

Minimum closeout evidence:

```text
mycelia stats --corpus mycelia --recent 20
```

If the slice did not use Mycelia, the closeout must say why. Valid reasons
include mandatory protocol file reads, exact line edits after Mycelia narrowed
the area, or a Mycelia miss. "I forgot" is a failure to record and fix.

Acceptance:

- `BUILD_STATE.md` includes a one-line dogfood result in slice validation notes.
- `AGENTS.md` Project Specifics names the dogfood gate.
- A future agent can tell from state whether the tool was used organically.

## Out of scope

- Ranking or retrieval-quality changes.
- Hard command interception.
- Mutation MCP tools.
- New top-level `doctor` command.
- Default project-source writes during `setup`.

## Validation

For the eventual implementation slice:

1. Standard Rust gates.
2. CLI help snapshot or focused assertions for hidden/visible commands.
3. Isolated `stats --all` fixture with two corpora: one active, one zero-use.
4. Isolated `connect --install-guidance` smoke if opt-in guidance is implemented.
5. Real stdio MCP exchange remains clean.
6. Dogfood evidence: this slice itself must use Mycelia for orientation and
   report recent stats.
