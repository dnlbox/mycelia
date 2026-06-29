# Phase 4 bakeoff — candelabrum-studio oracle (frozen)

Ground-truth findings for the PR-review bakeoff (agent **with** vs **without** Mycelia).
Target repo: `github.com/earlgreylabs/candelabrum-studio` (Bun + TS; Hono server, Run state machine, AI-SDK providers, React/Vite UI).

**Methodology (lead, 2026-06-29):**
- The **scored oracle is correctness issues only.** "Improvements"/nitpicks are recorded separately and are NOT counted in recall/false-positive scoring — because false-positive noise is the headline metric and nitpicks-as-findings would corrupt it.
- This oracle is established independently (lead analysis + manual verification) and **frozen before** running the A/B arms. The comparison (with vs without Mycelia, same PRs) plus token cost are the primary signals.
- Best cases are **cross-file**: the bug is only recognisable by tracing a contract through 2+ files — where change-scoped retrieval should help most.

## Correctness cases (scored)

### C1 — auto-resume drops director capability  ✅ lead-verified (HIGH, cross-file)
- **Primary:** `src/server/runtime.ts:90` — `resumeInterruptedRuns` calls `buildContext(settings, store, run)` with no `directorCapability`, defaulting to `finalise` (line 20). `canAutoResume` never includes `directing`, so every auto-resumed run gets `finalise`.
- **Cross-file proof:** `src/server/routes/runs.ts:192-202` builds a `capabilityByStatus` map and passes the derived capability; other routes pass explicit capabilities (lines 75/107/112/153). Auto-resume is the lone inconsistent call site.
- **Expected finding:** a captioning (or other) auto-resumed run resolves its director against the wrong provider-selection capability.
- **Why it's a good case:** invisible without comparing `runtime.ts:90` to `routes/runs.ts:192-202` across files.

### C2 — `proposeConcepts` history never populated (dead de-dup)  ✅ lead-verified, soft (MED, cross-file)
- **Primary:** `src/stages/direct.ts:22-31` — only production caller of `proposeConcepts`; never passes `history`.
- **Cross-file:** `src/core/providers.ts:23` defines `history?: string[]`; `src/providers/llm/director-claude.ts:38,57-58` consumes it to make concepts "distinct from recent concepts". Feature is wired but never fed in production.
- **Expected finding:** the documented concept de-duplication never actually fires.
- Lower confidence: arguably a dead-feature/latent gap rather than an active bug.

### C3 — `SERVABLE_ARTIFACTS` omits `upscaledImage`  ⏳ needs confirmation (LOW, cross-file, latent)
- **Primary:** `src/server/routes/assets.ts:4-10`. **Cross-file:** `src/core/run.ts` (`runArtifactsSchema` includes `upscaledImage`), `src/stages/upscale.ts:81` (populates it).
- **Expected finding:** asset route 404s for an artifact the schema/stage produce. Latent (UI only requests `image` today).

## Improvements (recorded, NOT scored)
- Dead `onPayload` callback in image stage (`src/stages/image.ts:14` vs providers).
- Redundant `rm` in `src/core/store.ts:32-34` finally block.
- Weak manual-inbox polling (lexicographic pick + flat sleep) in `src/providers/image/manual-inbox.ts:29-58`.

## Status / gap
Verified cross-file correctness cases: **C1 (strong), C2 (soft), C3 (pending)**. The ≥5-paired-task ship-gate decision rule needs ~2 more solid cross-file correctness cases — either another targeted analysis pass, or (lead-flagged decision) promote improvements into the scored set, which weakens the false-positive metric.
