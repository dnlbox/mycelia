import { execFile as execFileCallback } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

const execFile = promisify(execFileCallback);

async function runCommand(cmd, args, opts = {}) {
  try {
    return await execFile(cmd, args, { maxBuffer: 10 * 1024 * 1024, ...opts });
  } catch (err) {
    const stdout = err.stdout ? `\nstdout:\n${err.stdout}` : '';
    const stderr = err.stderr ? `\nstderr:\n${err.stderr}` : '';
    throw new Error(`${cmd} ${args.join(' ')} failed (exit ${err.code})${stdout}${stderr}`);
  }
}

const ORACLE_CASES = [
  {
    id: 'C1',
    name: 'auto-resume drops director capability',
    changed_path: 'src/server/runtime.ts',
    expected_finding: 'resumeInterruptedRuns calls buildContext without directorCapability, defaulting to finalise instead of directing capability.',
    required_files: ['src/server/routes/runs.ts'],
    discriminators: ['directorcapability', 'finalise', 'directing'],
  },
  {
    id: 'C2',
    name: 'proposeConcepts history never populated',
    changed_path: 'src/stages/direct.ts',
    expected_finding: 'proposeConcepts is never passed history parameter, rendering concept deduplication dead.',
    required_files: ['src/providers/llm/director-claude.ts', 'src/core/providers.ts'],
    discriminators: ['history', 'dedup', 'distinct'],
  },
  {
    id: 'C3',
    name: 'SERVABLE_ARTIFACTS omits upscaledImage',
    changed_path: 'src/server/routes/assets.ts',
    expected_finding: 'assets route SERVABLE_ARTIFACTS omits upscaledImage, returning 404 when requested.',
    required_files: ['src/core/run.ts', 'src/stages/upscale.ts'],
    discriminators: ['servable_artifacts', 'upscaledimage', '404'],
  },
  {
    id: 'C4',
    name: 'recover() is permanently broken',
    changed_path: 'src/core/orchestrator.ts',
    expected_finding: 'recover() requires status === "failed", but runs never transition to failed (recordRunFailure keeps status unchanged).',
    required_files: ['src/core/run.ts', 'src/server/routes/runs.ts'],
    discriminators: ['recordrunfailure', 'unreachable', 'failed'],
  },
  {
    id: 'C5',
    name: 'per-style caption guidance ignored',
    changed_path: 'src/stages/caption.ts',
    expected_finding: 'caption stage calls director.caption without passing ctx.style, ignoring required per-style caption configuration.',
    required_files: ['src/core/config.ts', 'src/core/providers.ts'],
    discriminators: ['ctx.style', 'style parameter', 'caption configuration'],
  },
  {
    id: 'C6',
    name: 'video providers return undefined payload on resume',
    changed_path: 'src/providers/video/fal.ts',
    expected_finding: 'on resume (when existingJobId set), provider returns payload as undefined because payload is only assigned on fresh submits.',
    required_files: ['src/stages/animate.ts', 'src/providers/video/veo.ts'],
    discriminators: ['existingjobid', 'payload', 'resume'],
  },
];

function evaluateReview(text, testCase) {
  if (!text || text.trim() === '' || text.toLowerCase().includes('no correctness issues found')) {
    return { recall: 0, falsePositivesClaimed: 0, notes: 'No correctness issues identified.' };
  }
  const lowerText = text.toLowerCase();
  const matchedDiscriminators = testCase.discriminators.filter((d) => lowerText.includes(d.toLowerCase()));
  const recall = matchedDiscriminators.length >= 2 ? 1 : 0;
  return {
    recall,
    falsePositivesClaimed: 0,
    notes: recall ? 'Expected finding identified.' : 'Review generated claims but missed expected core finding.',
  };
}

const repoRoot = resolve(process.cwd());
const localBin = join(process.env.HOME || '', '.local/bin/mycelia');
const myceliaBin = process.env.MYCELIA_BIN ?? (existsSync(localBin) ? localBin : 'mycelia');
const reviewAgentScript = join(repoRoot, 'examples/ai-sdk/review-agent.mjs');
const targetRepoUrl = process.env.TARGET_REPO_URL ?? 'https://github.com/earlgreylabs/candelabrum-studio.git';
const argTarget = process.argv.includes('--target') ? process.argv[process.argv.indexOf('--target') + 1] : null;
const isDryRun = process.argv.includes('--dry-run') || (!process.env.AI_GATEWAY_API_KEY && !process.env.ANTHROPIC_API_KEY && !process.env.OPENAI_API_KEY);

async function main() {
  console.log(`Starting Phase 4 Bakeoff Runner (${isDryRun ? 'DRY-RUN / HARNESS VALIDATION' : 'LIVE MODEL BAKEOFF'})`);
  let targetDir = argTarget ? resolve(argTarget) : null;
  let cleanupTarget = false;

  if (!targetDir || !existsSync(targetDir)) {
    console.log(`Cloning target repo ${targetRepoUrl}...`);
    targetDir = await mkdtemp(join(tmpdir(), 'candelabrum-bakeoff-'));
    cleanupTarget = true;
    await runCommand('git', ['clone', '--depth', '1', targetRepoUrl, targetDir]);
  } else {
    console.log(`Using target repo at ${targetDir}`);
  }

  try {
    console.log(`Preparing Mycelia index on target repo...`);
    await runCommand(myceliaBin, ['ci', 'prepare', '--no-embed', targetDir], { cwd: targetDir });

    const caseResults = [];
    let totalMyceliaTokens = 0;
    let totalBaselineTokens = 0;
    let myceliaRecallHits = 0;
    let baselineRecallHits = 0;
    let myceliaFalsePositives = 0;
    let baselineFalsePositives = 0;

    for (const testCase of ORACLE_CASES) {
      console.log(`\nRunning case ${testCase.id}: ${testCase.name}`);
      const changedFile = join(targetDir, testCase.changed_path);
      if (!existsSync(changedFile)) {
        console.warn(`Warning: changed path ${testCase.changed_path} does not exist in target repo. Skipping.`);
        continue;
      }

      for (const arm of ['mycelia', 'baseline']) {
        const disableFlag = arm === 'baseline' ? '1' : '0';
        console.log(`  Executing arm: ${arm.toUpperCase()}`);

        if (isDryRun) {
          console.log(`    [Dry-run] Verified target path ${testCase.changed_path} ready for ${arm} arm.`);
          caseResults.push({
            case: testCase.id,
            arm,
            status: 'dry-run-ok',
            tokens: 0,
            recall: 0,
            false_positives: 0,
            lead_judged_recall: null,
            lead_judged_false_positives: null,
          });
          continue;
        }

        const startTime = Date.now();
        try {
          const { stdout } = await runCommand('node', [reviewAgentScript, '--json', testCase.changed_path], {
            cwd: targetDir,
            env: {
              ...process.env,
              MYCELIA_PROJECT_ROOT: targetDir,
              MYCELIA_BIN: myceliaBin,
              MYCELIA_DISABLE: disableFlag,
            },
          });
          const elapsed = Date.now() - startTime;
          let parsed;
          try {
            parsed = JSON.parse(stdout.trim());
          } catch {
            parsed = { text: stdout, usage: { totalTokens: 0 }, stepsCount: 0 };
          }

          const tokens = parsed.usage?.totalTokens || 0;
          const reviewText = parsed.text || '';
          const evalRes = evaluateReview(reviewText, testCase);

          if (arm === 'mycelia') {
            totalMyceliaTokens += tokens;
            myceliaRecallHits += evalRes.recall;
            myceliaFalsePositives += evalRes.falsePositivesClaimed;
          } else {
            totalBaselineTokens += tokens;
            baselineRecallHits += evalRes.recall;
            baselineFalsePositives += evalRes.falsePositivesClaimed;
          }

          caseResults.push({
            case: testCase.id,
            arm,
            elapsed_ms: elapsed,
            tokens,
            steps: parsed.stepsCount,
            recall: evalRes.recall,
            false_positives: evalRes.falsePositivesClaimed,
            review_text: reviewText,
            lead_judged_recall: null,
            lead_judged_false_positives: null,
          });
        } catch (err) {
          console.error(`    Error running ${arm} on ${testCase.id}: ${err.message}`);
          caseResults.push({ case: testCase.id, arm, error: err.message });
        }
      }
    }

    const tokenReductionRatio =
      totalBaselineTokens > 0 ? (totalBaselineTokens - totalMyceliaTokens) / totalBaselineTokens : 0;
    const decisionRuleMet =
      tokenReductionRatio >= 0.25 &&
      myceliaRecallHits >= baselineRecallHits &&
      myceliaFalsePositives <= baselineFalsePositives;

    const report = {
      cases: caseResults,
      summary: isDryRun
        ? { status: 'dry-run-complete', cases_checked: ORACLE_CASES.length }
        : {
            total_mycelia_tokens: totalMyceliaTokens,
            total_baseline_tokens: totalBaselineTokens,
            token_reduction_ratio: tokenReductionRatio,
            mycelia_recall: `${myceliaRecallHits}/${ORACLE_CASES.length}`,
            baseline_recall: `${baselineRecallHits}/${ORACLE_CASES.length}`,
            mycelia_false_positives: myceliaFalsePositives,
            baseline_false_positives: baselineFalsePositives,
            decision_rule_met: decisionRuleMet,
          },
    };

    console.log('\n--- Bakeoff Report ---');
    console.log(JSON.stringify(report, null, 2));
  } finally {
    if (cleanupTarget) {
      await rm(targetDir, { recursive: true, force: true }).catch(() => {});
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
