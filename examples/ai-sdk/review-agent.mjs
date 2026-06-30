import { execFile as execFileCallback } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { resolve, relative } from 'node:path';
import { promisify } from 'node:util';

import { createMCPClient } from '@ai-sdk/mcp';
import { Experimental_StdioMCPTransport } from '@ai-sdk/mcp/mcp-stdio';
import { ToolLoopAgent, stepCountIs, tool } from 'ai';
import { z } from 'zod';

const execFile = promisify(execFileCallback);

const projectRoot = process.env.MYCELIA_PROJECT_ROOT ?? process.cwd();
const myceliaBin = process.env.MYCELIA_BIN ?? 'mycelia';
const model = process.env.AI_GATEWAY_MODEL ?? 'anthropic/claude-haiku-4-5';
const changedPathFile = process.env.MYCELIA_CHANGED_PATHS;
const disableMycelia = process.env.MYCELIA_DISABLE === '1' || process.env.MYCELIA_DISABLE === 'true';

const outputJson = process.argv.includes('--json') || process.env.MYCELIA_OUTPUT_JSON === '1';

const changedPaths = changedPathFile
  ? (await readFile(changedPathFile, 'utf8')).split(/\r?\n/).filter(Boolean)
  : process.argv.slice(2).filter((arg) => arg !== '--json');

if (changedPaths.length === 0) {
  throw new Error(
    'Pass changed paths as argv or set MYCELIA_CHANGED_PATHS to a newline-delimited file.',
  );
}

let tools;
let instructions;
let mcpClient = null;

if (disableMycelia) {
  tools = {
    read_file: tool({
      description: 'Read the contents of a file at a relative path in the codebase.',
      inputSchema: z.object({
        path: z.string().describe('Relative path to the file from repository root'),
        startLine: z.number().optional().describe('Optional 1-indexed start line number'),
        endLine: z.number().optional().describe('Optional 1-indexed end line number'),
      }),
      execute: async ({ path, startLine, endLine }) => {
        const absPath = resolve(projectRoot, path);
        if (relative(projectRoot, absPath).startsWith('..')) {
          return 'Error: Path traversal outside project root is not allowed.';
        }
        try {
          const content = await readFile(absPath, 'utf8');
          const lines = content.split(/\r?\n/);
          const start = startLine ? Math.max(1, startLine) - 1 : 0;
          const end = endLine ? Math.min(lines.length, endLine) : lines.length;
          const sliced = lines.slice(start, end);
          if (sliced.length > 500) {
            return sliced.slice(0, 500).join('\n') + '\n[Truncated after 500 lines]';
          }
          return sliced.join('\n');
        } catch (err) {
          return `Error reading file: ${err.message}`;
        }
      },
    }),
    grep_search: tool({
      description: 'Search for a literal string or pattern across files in the codebase.',
      inputSchema: z.object({
        query: z.string().describe('Search query string'),
        pathPrefix: z.string().optional().describe('Optional directory or path prefix to search inside'),
      }),
      execute: async ({ query, pathPrefix }) => {
        try {
          const args = ['grep', '-n', '-I', query, '--', pathPrefix || '.'];
          const { stdout } = await execFile('git', args, { cwd: projectRoot, maxBuffer: 1024 * 1024 });
          const lines = stdout.trim().split(/\r?\n/).filter(Boolean);
          if (lines.length > 50) {
            return lines.slice(0, 50).join('\n') + `\n[Truncated: ${lines.length - 50} more matches]`;
          }
          return lines.join('\n');
        } catch (err) {
          if (err.code === 1) {
            return 'No matches found.';
          }
          return `Error searching: ${err.message}`;
        }
      },
    }),
  };
  instructions = [
    'You are a headless PR-review agent on a STRICT tool budget. Be decisive, not exhaustive.',
    'Procedure:',
    '1. Read the changed files or grep for related usages AT MOST 5 times total.',
    '2. Then STOP calling tools and write the review.',
    'Output: a concise review of likely CORRECTNESS issues only (no style/nits).',
    'If none, say "No correctness issues found." Always produce the review within budget; never keep exploring.',
  ].join('\n');
} else {
  mcpClient = await createMCPClient({
    clientName: 'mycelia-review-agent',
    version: '0.1.0',
    transport: new Experimental_StdioMCPTransport({
      command: myceliaBin,
      args: ['serve', '--project-root', projectRoot, '--lexical'],
      cwd: projectRoot,
      env: process.env,
      stderr: 'pipe',
    }),
  });
  tools = await mcpClient.tools();
  instructions = [
    'You are a headless PR-review agent on a STRICT tool budget. Be decisive, not exhaustive.',
    'Procedure:',
    '1. Call find_changed ONCE with all changed paths to get the blast radius.',
    '2. Retrieve AT MOST 4 chunks — only the ones most likely to hide a correctness bug.',
    '3. Then STOP calling tools and write the review.',
    'Output: a concise review of likely CORRECTNESS issues only (no style/nits).',
    'If none, say "No correctness issues found." Always produce the review within budget; never keep exploring.',
  ].join('\n');
}

try {
  const agent = new ToolLoopAgent({
    model,
    tools,
    stopWhen: stepCountIs(8),
    instructions,
  });

  const result = await agent.generate({
    prompt: [
      disableMycelia
        ? 'Review this pull request using available tools for code context.'
        : 'Review this pull request using Mycelia for code context.',
      '',
      'Changed paths:',
      ...changedPaths.map((path) => `- ${path}`),
    ].join('\n'),
  });

  if (outputJson) {
    console.log(
      JSON.stringify({
        text: result.text || '',
        usage: result.usage || { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
        stepsCount: Array.isArray(result.steps) ? result.steps.length : 0,
      }),
    );
  } else {
    console.log(result.text);
  }
} finally {
  if (mcpClient) {
    await mcpClient.close();
  }
}
