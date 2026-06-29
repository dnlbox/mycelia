import { execFile as execFileCallback } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
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
    expected_keywords: ['capability', 'directorCapability', 'finalise', 'autoResume'],
    required_files: ['src/server/routes/runs.ts'],
  },
  {
    id: 'C2',
    name: 'proposeConcepts history never populated',
    changed_path: 'src/stages/direct.ts',
    expected_keywords: ['history', 'proposeConcepts', 'distinct'],
    required_files: ['src/providers/llm/director-claude.ts', 'src/core/providers.ts'],
  },
  {
    id: 'C3',
    name: 'SERVABLE_ARTIFACTS omits upscaledImage',
    changed_path: 'src/server/routes/assets.ts',
    expected_keywords: ['upscaledImage', 'SERVABLE_ARTIFACTS', '404'],
    required_files: ['src/core/run.ts', 'src/stages/upscale.ts'],
  },
  {
    id: 'C4',
    name: 'recover() is permanently broken',
    changed_path: 'src/core/orchestrator.ts',
    expected_keywords: ['recover', 'failed', 'status', 'recordRunFailure'],
    required_files: ['src/core/run.ts', 'src/server/routes/runs.ts'],
  },
  {
    id: 'C5',
    name: 'per-style caption guidance ignored',
    changed_path: 'src/stages/caption.ts',
    expected_keywords: ['style', 'caption'],
    required_files: ['src/core/config.ts', 'src/core/providers.ts'],
  },
  {
    id: 'C6',
    name: 'video providers return undefined payload on resume',
    changed_path: 'src/providers/video/fal.ts',
    expected_keywords: ['payload', 'resume', 'existingJobId', 'undefined'],
    required_files: ['src/stages/animate.ts', 'src/providers/video/veo.ts'],
  },
];

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

    const results = [];
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
          results.push({ case: testCase.id, arm, status: 'dry-run-ok' });
          continue;
        }

        const startTime = Date.now();
        try {
          const { stdout } = await runCommand('node', [reviewAgentScript, testCase.changed_path], {
            cwd: targetDir,
            env: {
              ...process.env,
              MYCELIA_PROJECT_ROOT: targetDir,
              MYCELIA_BIN: myceliaBin,
              MYCELIA_DISABLE: disableFlag,
            },
          });
          const elapsed = Date.now() - startTime;
          const hitKeywords = testCase.expected_keywords.filter((kw) => stdout.toLowerCase().includes(kw.toLowerCase()));
          const hitRate = hitKeywords.length > 0 ? 1.0 : 0.0;
          results.push({
            case: testCase.id,
            arm,
            elapsed_ms: elapsed,
            hit_rate: hitRate,
            output_preview: stdout.slice(0, 200).replace(/\n/g, ' '),
          });
        } catch (err) {
          console.error(`    Error running ${arm} on ${testCase.id}: ${err.message}`);
          results.push({ case: testCase.id, arm, error: err.message });
        }
      }
    }

    console.log('\n--- Bakeoff Summary ---');
    console.log(JSON.stringify(results, null, 2));
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
