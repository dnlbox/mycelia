# Phase B2 interactive measurement

Date started: 2026-06-28

## Purpose

This is the Phase B publish-or-shelf gate for the v2 interactive path. The
question is narrow:

Does a project-attached Mycelia setup make real interactive agent sessions reach
the right files with fewer orientation tokens than the default grep/read reflex?

The gate is not satisfied by a healthy MCP server, a good retrieval eval, or a
single local anecdote. It needs paired harness runs.

## Harnesses

Primary:

- Codex
- Claude Code

Secondary, after B1 wiring is installed and confirmed:

- Antigravity
- OpenCode
- Kilo

## Variants

Each task gets two fresh sessions per harness.

Baseline:

- No Mycelia guidance in the project instruction path.
- Mycelia MCP unavailable or not mentioned.
- The agent can use normal shell search and file reads.

Mycelia:

- B1 guidance is installed through that harness's normal project convention.
- The Mycelia MCP server is connected and loaded.
- The prompt does not explicitly force Mycelia. Organic use is part of the
  measurement.
- Claimed Mycelia use counts only when the transcript contains an actual Mycelia
  MCP tool-call record. Shell commands such as `rg`, `grep`, `sed`, `nl`, or
  direct file reads do not count, even if the agent says "Mycelia found" in prose.

Both variants use the same user prompt, repository checkout, and time budget.
Do not tell the agent the expected files.

## Task Set

### B2-T1: session-state orientation

Prompt:

```text
Orient on the current Mycelia slice and tell me what the next concrete step is.
Do not change files.
```

Expected files:

- `BUILD_STATE.md`
- `ROADMAP.md`
- `docs/concept/v2/00_vision.md`
- `docs/concept/v2/06_visibility_and_diagnostics.md`
- `docs/concept/24_dogfood_and_protocol_adoption.md`

Success:

- Names Phase B / Slice B2 as active.
- Explains that B2 is a measurement gate, not a production-code slice.
- Does not claim a publish decision without paired evidence.

### B2-T2: guidance-plane implementation hunt

Prompt:

```text
Find where Mycelia writes project guidance for harnesses and summarize how the
owned block stays idempotent.
```

Expected files:

- `crates/mycelia-cli/src/project.rs`
- `crates/mycelia-cli/src/main.rs`
- `crates/mycelia-cli/tests/cli.rs`

Success:

- Identifies `detect_guidance_files`, `guidance_include_preview`, and
  `insert_guidance_include`.
- Identifies the Claude settings special case.
- Cites tests for idempotence and harness detection.

### B2-T3: MCP surface orientation

Prompt:

```text
Find the MCP server surface and explain which tools are model-facing.
```

Expected files:

- `crates/mycelia-cli/src/mcp.rs`
- `crates/mycelia-cli/src/main.rs`
- `crates/mycelia-cli/tests/cli.rs`
- `docs/concept/22_multi_corpus_server.md`
- `docs/concept/v2/07_requirements_carry_forward.md`

Success:

- Names `find`, `retrieve`, aliases, `find_related`, and corpus/project listing.
- Confirms the surface remains read-only.
- Confirms stdout is reserved for protocol.

### B2-T4: freshness guarantee hunt

Prompt:

```text
Find the code and docs for the stale-slice guarantee. Explain what retrieve
returns when a source file changed after indexing.
```

Expected files:

- `crates/mycelia-core/src/store.rs`
- `crates/mycelia-cli/src/mcp.rs`
- `docs/concept/20_freshness_and_staleness.md`
- `docs/concept/v2/07_requirements_carry_forward.md`

Success:

- States that Mycelia never serves a stale indexed slice.
- Correctly distinguishes `ok`, `file`, and `unavailable`.
- Identifies query-time self-heal behavior.

### B2-T5: adoption observability hunt

Prompt:

```text
Find the stats and status surfaces and explain what evidence shows whether an
agent actually used Mycelia.
```

Expected files:

- `crates/mycelia-cli/src/log.rs`
- `crates/mycelia-cli/src/main.rs`
- `docs/concept/19_user_journey_and_observability.md`
- `docs/concept/24_dogfood_and_protocol_adoption.md`
- `docs/concept/v2/06_visibility_and_diagnostics.md`

Success:

- Identifies `stats` as the adoption and token-value dashboard.
- Uses recent `find` and `retrieve` activity as evidence.
- Does not propose a competing `doctor adoption` command.

## Metrics

Record these per task, harness, and variant:

- first broad-orientation tool: Mycelia, shell search, direct file read, or other
- transcript-visible Mycelia MCP tool calls, by name
- Mycelia `find` calls before first expected file
- Mycelia `retrieve` calls before first expected file
- shell search commands before first expected file
- direct file reads before first expected file
- unique files opened before first expected file
- unique files opened total
- wall-clock seconds to first expected file
- wall-clock seconds to correct summary
- transcript token count to first expected file when the harness exposes it
- fallback token estimate when transcript tokens are unavailable: bytes of file
  and command output read before first expected file divided by 4
- Mycelia log delta from `mycelia stats --corpus mycelia --recent 20`
- pass/fail against the task success criteria

For Mycelia runs, also record logged `actual_tok`, `cold_tok`, and savings ratio
for each `find`.

If the agent claims Mycelia use but the transcript has no Mycelia MCP tool-call
record, mark `first_broad_orientation_tool` as the actual observed tool and mark
the run as a guidance failure.

## Decision Rule

Proceed past Phase B only if both primary harnesses satisfy all of these:

- Organic adoption: Mycelia is the first broad-orientation path in at least four
  of five tasks.
- Token improvement: median tokens to first expected file improves by at least
  25 percent versus baseline.
- Correctness: Mycelia does not reduce task success rate.
- Latency: median time to correct summary is not more than 20 percent worse.
- Operator trust: no run requires hidden project mutation or machine-level
  changes beyond `connect`.

If the primary harnesses miss those bars, stop Phase B and write a
shelf-or-narrow recommendation. Do not continue into Phase C hardening to avoid
answering the wrong question.

## Current Codex Observation

This kickoff session is not a paired B2 run because it included mandatory
protocol reads and no controlled baseline. It is still useful as a guardrail.

Observed:

- Mandatory direct reads were appropriate for `AGENTS.md`, `prompt.md`,
  `BUILD_STATE.md`, and the v2 spine because those files define the session
  contract.
- `mycelia stats --corpus mycelia --recent 20` reported 22 answered queries,
  about 17,788 tokens via Mycelia, about 542,647 cold-read tokens, and estimated
  30.5x savings before this document was written.
- A Mycelia query for B2 roadmap context returned mostly MCP/code-surface hits
  and an `AGENTS.md` dogfood chunk, but did not surface `ROADMAP.md`.
- `mycelia status --corpus mycelia` reported 1,553 chunks and incomplete
  embeddings, 1,345 / 1,553, with the suggested fix `mycelia refresh`.

Interpretation:

The current Codex session does not prove or disprove the B2 claim. It shows that
mandatory state files should still be read directly, and that the named Mycelia
corpus should be refreshed before using it for the controlled measurement runs.

## Run Log Template

```text
run_id:
date:
harness:
variant: baseline | mycelia
task_id:
repo_commit:
index_refreshed: yes | no | n/a
prompt:
first_broad_orientation_tool:
mycelia_find_before_first_expected:
mycelia_retrieve_before_first_expected:
shell_search_before_first_expected:
direct_reads_before_first_expected:
unique_files_before_first_expected:
unique_files_total:
tokens_to_first_expected:
token_count_source: harness | bytes_div_4
seconds_to_first_expected:
seconds_to_correct_summary:
mycelia_stats_delta:
visible_mycelia_mcp_calls:
expected_files_hit:
success:
notes:
```
