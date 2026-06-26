# Session kickstart

Static, do not edit per session. The build intent lives in `docs/concept/`; the
current state lives in `BUILD_STATE.md`.

## Start protocol

1. Read `AGENTS.md`, then inventory `.agents/`.
2. Read `BUILD_STATE.md`. Reconcile `Now` with `git log --oneline -10` and the
   validation state. Correct stale state before continuing.
3. Exercise the last checkpoint.
4. Continue from `Now`'s next step. Record intent before each slice, verify it,
   then checkpoint. Never end on a broken tree.

After changing `docs/concept/`, reconcile the Project Specifics in `AGENTS.md`
before implementing the changed decision.
