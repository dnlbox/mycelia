import { readFile } from 'node:fs/promises';
import { createMCPClient } from '@ai-sdk/mcp';
import { Experimental_StdioMCPTransport } from '@ai-sdk/mcp/mcp-stdio';
import { ToolLoopAgent, stepCountIs } from 'ai';

const projectRoot = process.env.MYCELIA_PROJECT_ROOT ?? process.cwd();
const myceliaBin = process.env.MYCELIA_BIN ?? 'mycelia';
const model = process.env.AI_GATEWAY_MODEL ?? 'anthropic/claude-sonnet-4-5';
const changedPathFile = process.env.MYCELIA_CHANGED_PATHS;

const changedPaths = changedPathFile
  ? (await readFile(changedPathFile, 'utf8')).split(/\r?\n/).filter(Boolean)
  : process.argv.slice(2);

if (changedPaths.length === 0) {
  throw new Error(
    'Pass changed paths as argv or set MYCELIA_CHANGED_PATHS to a newline-delimited file.',
  );
}

const mcpClient = await createMCPClient({
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

try {
  const tools = await mcpClient.tools();
  const agent = new ToolLoopAgent({
    model,
    tools,
    stopWhen: stepCountIs(8),
    instructions: [
      'You are a headless PR-review agent on a STRICT tool budget. Be decisive, not exhaustive.',
      'Procedure:',
      '1. Call find_changed ONCE with all changed paths to get the blast radius.',
      '2. Retrieve AT MOST 4 chunks — only the ones most likely to hide a correctness bug.',
      '3. Then STOP calling tools and write the review.',
      'Output: a concise review of likely CORRECTNESS issues only (no style/nits).',
      'If none, say "No correctness issues found." Always produce the review within budget; never keep exploring.',
    ].join('\n'),
  });

  const { text } = await agent.generate({
    prompt: [
      'Review this pull request using Mycelia for code context.',
      '',
      'Changed paths:',
      ...changedPaths.map((path) => `- ${path}`),
    ].join('\n'),
  });

  console.log(text);
} finally {
  await mcpClient.close();
}
