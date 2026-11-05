#!/usr/bin/env node
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js';
import { loadConfig, parseArguments } from '../client/config.ts';
import { createMcpServer } from './index.ts';

async function main(): Promise<void> {
  const args = parseArguments(process.argv.slice(2));
  if (args.version) { process.stdout.write('athenaeum-mcp 0.1.0\n'); return; }
  if (args.help) {
    process.stdout.write(
      'Usage: athenaeum-mcp [--repo NAME=PATH] [--allow propose,review,sync] [--config FILE] [--athenaeum COMMAND] [--caller NAME] [--session ID]\n' +
        '\n' +
        'Reads are always available. `propose` lets a model queue knowledge for review, `sync` lets it\n' +
        'sync accepted wiki text with the Library, and `review` lets it accept or reject a proposal.\n' +
        'Every accept and reject also needs an explicit reviewer identity on the call.\n',
    );
    return;
  }
  const server = createMcpServer(loadConfig(args));
  await server.connect(new StdioServerTransport());
}

main().catch(error => { process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`); process.exitCode = 1; });
