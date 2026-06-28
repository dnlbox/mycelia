# Lessons

These are the reusable lessons from the Mycelia journey.

## 1. Prompting is product design

Prompts are not just text. In an agent workflow, prompts decide:

- what tools the model notices
- what "done" means
- what evidence must be collected
- what tradeoffs are allowed

The strongest prompts in this project were specific:

- read protocol files first
- do not change files yet
- use bounded subagents with disjoint file areas
- verify through the built binary
- count transcript-visible MCP calls only

Vague prompts produced vague behavior. Precise prompts created measurable work.

## 2. Harnesses have their own gravity

Codex, Claude Code, Claude Desktop, Antigravity, OpenCode, Kilo, and Cursor do
not behave like neutral shells around the same model.

Each harness has its own:

- instruction files
- tool loading behavior
- native search tools
- MCP discovery path
- default habits

This matters because Mycelia competes with the harness's built-in tools. If grep
is always available and MCP tools are deferred, the model may choose grep even
when Mycelia would save tokens.

## 3. MCP is a protocol, not adoption

The team learned this the hard way.

An MCP server can be installed, listed, and healthy, but still unused.

Good adoption needs at least four things:

- the tool is connected
- the tool schema is visible at decision time
- the model receives clear guidance
- the workflow rewards using the tool

Without those, MCP is just potential energy.

## 4. A two-stage tool surface is a strong default

`find` and `retrieve` became one of the core product shapes.

The pattern is:

```text
find: cheap headers
retrieve: selected body
```

This is better than returning full text from search because it lets the model
look before spending tokens.

The next refinement is important: sometimes the harness already has a cheap
line-range file viewer. If that tool returns only the needed lines, forcing
`retrieve` may not save tokens. The real goal is not "use retrieve." The real
goal is "avoid reading whole files when a smaller slice is enough."

## 5. CLI design is a teaching tool

The CLI started as diagnostic commands. It became a beginner journey.

Good beginner verbs:

- `setup`: make this repo usable
- `connect`: wire my harness
- `status`: tell me if it works
- `stats`: tell me if it saves tokens
- `refresh`: update the index
- `list`: show what exists
- `delete`: clean it up

Bad beginner experience:

- making users think in raw database paths
- making `serve` feel like a normal command
- hiding important downloads inside install
- requiring users to know MCP internals

The best CLI commands explain the product without a tutorial.

## 6. Rust helped because the contracts were sharp

Rust was a good fit because Mycelia cares about:

- deterministic data
- explicit errors
- careful file handling
- strict boundaries between core and transport
- no stale or fake source slices

The project also avoided premature Rust complexity:

- no async in core
- no unsafe
- no custom storage
- no approximate vector index before measurement
- narrow traits before broad plugin systems

That restraint kept the code easier to reason about.

## 7. Measurement beat vibes

Several tempting ideas did not win:

- reciprocal-rank fusion
- making semantic hybrid the default too early
- adopting Graphify outright
- treating MCP setup as enough proof

They lost because the project had gates:

- hit rate
- MRR
- tokens per answered query
- CLI smoke tests
- MCP smoke tests
- transcript-visible tool calls

The most important metric was tokens per answered query. Mycelia exists to save
model context, so any change that increases answer tokens must justify itself.

## 8. Freshness is trust

If a model receives stale code, it may confidently explain something false.

Mycelia's freshness rule became:

```text
Never serve an indexed chunk if the source changed.
```

That single rule made several design choices obvious:

- validate on `retrieve`
- validate top `find` results
- self-heal drifted sources
- return live files when precise chunks are no longer safe
- mark deleted or unreadable sources as unavailable

Correctness matters more than cache purity.

## 9. Competitor research should reduce panic

Graphify and codebase-memory-mcp were not threats to ignore. They were mirrors.

They showed:

- graph ideas are valuable
- install experience matters
- user-level integration can feel easy but become invasive
- speed and artifact reuse matter
- adoption is often harder than retrieval

The right response was not "stop building" or "copy them." It was "find the
specific wedge Mycelia can own."

## 10. Pivoting is not quitting

The project almost became "build a better indexer."

Then the real product problem appeared:

```text
How do we make agents actually use the better context path?
```

That pivot kept the work honest. V1 retrieval work was not wasted. It became the
engine under a better v2 product question.

Perseverance was useful while the unknown was technical. Pivoting was necessary
when the unknown became behavioral.

## 11. Subagents need hard boundaries

Subagents worked best when they had:

- one task
- one file area
- no decision authority
- clear verification instructions

They worked poorly when long-running tasks sprawled or when the main agent did
not re-verify output.

The lesson for junior developers: delegation is not ownership transfer. The
main loop still owns integration and correctness.

## 12. Humans are part of the system

The project improved because assumptions were challenged:

- "Maybe Graphify makes this unnecessary."
- "Maybe stale chunks break trust."
- "Maybe stats matters more than another command."
- "Maybe MCP is installed but unused."
- "Maybe project-level context is the real product."

That is the guardrail pattern worth keeping. The user can be wrong, the agent can
be wrong, and the process needs evidence strong enough to catch both.
