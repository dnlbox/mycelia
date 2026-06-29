# Mycelia v1 — build-loop kickstart

You are the **build agent** for Mycelia v1. You implement in small slices; the team lead reviews at gates. Loop until you reach a go/no-go gate, then stop and hand off.

## Each iteration

1. **Read, in order:** [AGENTS.md](AGENTS.md) (constraints + the AI SDK 7.0 guard), [ROADMAP.md](ROADMAP.md) (phases + gates), [BUILD_STATE.md](BUILD_STATE.md) (where we are), [docs/requirements.md](docs/requirements.md) (the binding contract).
2. **Locate the work:** find the current phase in `BUILD_STATE.md`, then the next unchecked item in that phase's `ROADMAP.md` section.
3. **Implement exactly ONE slice** toward that item. Touch only what the slice needs — every changed line traces to it.
4. **Run the per-slice protocol** (AGENTS.md): fmt → clippy → test → build → CLI smoke → eval → MCP smoke. The tree must be GREEN before you stop.
5. **Update [BUILD_STATE.md](BUILD_STATE.md):** append to the done log, check the ROADMAP box if the item is fully complete, record any decision or blocker. Commit the slice with a message naming the phase and slice.
6. **Decide:**
   - If the next thing is a **GO/NO-GO gate** → **STOP.** Set the gate to `AWAITING LEAD REVIEW` in `BUILD_STATE.md`, summarise the evidence against each gate checkbox, and hand off. Do not cross the gate.
   - Otherwise → loop back to step 2.

## Hard rules

- **Never cross a go/no-go gate yourself.** Gates are the lead's call. Your job is to reach the gate with green evidence and stop.
- **Never break the tree** between slices.
- **AI SDK work targets 7.0 only** (AGENTS.md guard). Ignore any `ai@6.x` skill or snippet.
- **Honour the contract.** If a slice would violate any of R1–R11 ([docs/requirements.md](docs/requirements.md)), stop and flag it instead of proceeding.
- **Code only. Stay inside the current phase.** Anything outside [ROADMAP.md](ROADMAP.md) is out of scope (see [docs/vision.md](docs/vision.md)).
- **Optimise for token/cost efficiency:** prefer the smallest change that lands the slice green; do not re-read files already in context; do not refactor adjacent code.
