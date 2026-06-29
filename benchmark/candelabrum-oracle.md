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

### C4 — `recover()` is permanently broken; `"failed"` status unreachable  ✅ lead-verified (HIGH, cross-file)
- **Primary:** `src/core/orchestrator.ts:206-219` — `recover()` throws unless `run.status === "failed"`.
- **Cross-file proof:** `src/core/run.ts:223-245` `recordRunFailure` deliberately keeps `run.status` unchanged and pushes a `stage_error` event with `to: run.status` (never `"failed"`); a repo-wide grep confirms NO code path ever transitions a run to `"failed"`. So `POST /api/runs/:id/recover` (`src/server/routes/runs.ts:211-226`) always throws for any genuinely-failed run; `"failed"` (in `TERMINAL_STATUSES`, run.ts:39) is unreachable.
- **Expected finding:** recover is dead — failed runs can never be recovered. Strongest case alongside C1.

### C5 — per-style `caption` guidance required, validated, and ignored  ✅ lead-verified (MED, cross-file)
- **Primary:** `src/stages/caption.ts:23` — calls `ctx.director.caption(run.shotSpec, platform, cb)`, never passes `ctx.style`.
- **Cross-file:** `src/core/providers.ts:41` `caption(shotSpec, platform, onPayload?)` has no `style` param; `src/core/config.ts:68-71` makes `style.caption.{tiktok,instagram}` REQUIRED and every style TOML populates it. Dead config — every other director call uses `style`, this one drops it.
- **Expected finding:** platform caption directives never reach the captioner.

### C6 — Fal/Veo video providers return `payload: undefined` on resume  ✅ lead-verified (MED, cross-file)
- **Primary:** `src/providers/video/fal.ts:130,142,170` — `payload` is assigned only inside the `if (!requestId)` fresh-submit branch; on resume (`existingJobId` set) it stays `undefined` and is returned as-is.
- **Cross-file:** `src/stages/animate.ts:37-43` pushes `artifact.payload` (undefined) into the cost ledger (`run.ts` `costEntrySchema` allows optional payload, so it is silent). Same gap in `src/providers/video/veo.ts`.
- **Expected finding:** resumed video runs have no record of the submitted provider/model input in the cost audit.

## Improvements (recorded, NOT scored)
- Dead `onPayload` callback in image stage (`src/stages/image.ts:14` vs providers).
- Redundant `rm` in `src/core/store.ts:32-34` finally block.
- Weak manual-inbox polling (lexicographic pick + flat sleep) in `src/providers/image/manual-inbox.ts:29-58`.

## Status
**Oracle READY — ≥5 met.** Six cross-file correctness cases, all lead-verified except C3: **C1 + C4 (HIGH)**, **C2 + C5 + C6 (MED)**, C3 (LOW, pending — bonus 6th). Improvements remain unscored. Satisfies the ≥5-paired-task ship-gate rule. Each case → one bakeoff PR touching the finding's area; score **recall** (did the arm flag the expected finding), **false positives** (flagged issues NOT in the oracle), and **tokens**, agent WITH vs WITHOUT Mycelia.
