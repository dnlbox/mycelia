import { execFile as execFileCallback } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { createMCPClient } from '@ai-sdk/mcp';
import { Experimental_StdioMCPTransport } from '@ai-sdk/mcp/mcp-stdio';

const execFile = promisify(execFileCallback);

async function run(command, args, options = {}) {
  try {
    return await execFile(command, args, options);
  } catch (error) {
    const stdout = error.stdout ? `\nstdout:\n${error.stdout}` : '';
    const stderr = error.stderr ? `\nstderr:\n${error.stderr}` : '';
    throw new Error(`${command} ${args.join(' ')} failed${stdout}${stderr}`);
  }
}

const mycelia = process.env.MYCELIA_BIN ?? 'mycelia';
const root = await mkdtemp(join(tmpdir(), 'mycelia-ai-sdk-'));

await run('git', ['init'], { cwd: root });
await run('git', ['config', 'user.email', 'mycelia@example.test'], { cwd: root });
await run('git', ['config', 'user.name', 'Mycelia Test'], { cwd: root });
await mkdir(join(root, 'src'), { recursive: true });
await writeFile(
  join(root, 'src', 'review.ts'),
  'export function reviewTarget(): string { return "needle"; }\n',
);
await run('git', ['add', '.'], { cwd: root });
await run('git', ['commit', '-m', 'initial'], { cwd: root });
await run(mycelia, ['ci', 'prepare', '--no-embed', root], { cwd: root });

const mcpClient = await createMCPClient({
  clientName: 'mycelia-ai-sdk-smoke',
  version: '0.1.0',
  transport: new Experimental_StdioMCPTransport({
    command: mycelia,
    args: ['serve', '--project-root', root, '--lexical'],
    cwd: root,
    env: process.env,
    stderr: 'pipe',
  }),
});

try {
  const listed = await mcpClient.listTools();
  const toolNames = listed.tools.map((tool) => tool.name).sort();
  for (const expected of [
    'find',
    'find_changed',
    'find_related',
    'list_corpora',
    'locate_implementation',
    'retrieve',
    'search_codebase',
  ]) {
    if (!toolNames.includes(expected)) {
      throw new Error(`missing MCP tool ${expected}; got ${toolNames.join(', ')}`);
    }
  }

  const tools = await mcpClient.tools();
  if (!tools.find?.inputSchema) {
    throw new Error('AI SDK converted MCP tool is missing inputSchema');
  }

  const result = await mcpClient.callTool({
    name: 'find',
    arguments: { query: 'reviewTarget', limit: 5 },
  });
  const text = result.content?.[0]?.text;
  if (typeof text !== 'string' || !text.includes('"source_path":"src/review.ts"')) {
    throw new Error(`find did not return the expected source header: ${JSON.stringify(result)}`);
  }

  console.log(
    JSON.stringify({
      status: 'ok',
      server: mcpClient.serverInfo.name,
      tools: toolNames,
      input_schema: true,
      source_path: 'src/review.ts',
    }),
  );
} finally {
  await mcpClient.close();
}
