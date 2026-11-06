#!/usr/bin/env node
import { loadConfig, parseArguments } from './client/config.ts';
import { mcpLaunch } from './adapters/index.ts';
import { AGENT_IDS, CAPTURE_MODES, connect, diagnose, disconnect, type AgentId, type AgentReport, type CaptureMode, type ConnectOptions } from './connect/index.ts';
import { HOOK_EVENTS, runHook, type HookEvent } from './hooks/index.ts';

const USAGE = `Usage: athenaeum-integrations <command>

  connect      Wire Athenaeum Agent, Claude Code and Cursor to Athenaeum (MCP server, and hooks where the agent has them)
  disconnect   Remove exactly what connect added
  doctor       Check every link between the agents and the knowledge base
  mcp          Run the MCP server on stdio (what the agents launch)
  hook         Run one agent hook (what Claude Code launches); never fails the agent
  (none)       Print an MCP configuration stanza for --repo NAME=PATH

connect / disconnect / doctor options:
  --agent athenaeum,claude,cursor   Which agents (default: the ones installed)
  --athenaeum PATH              The athenaeum command (default: ATHENAEUM_BIN, beside this script, PATH)
  --workspace PATH              Connect to this wiki even when the agent works in another folder (kept on reconnect)
  --cwd PATH                    Directory to diagnose (doctor only)
  --allow propose,sync,review   MCP grants for the agents (default: propose)
  --capture off|instruct|nudge  How actively agents are asked to record knowledge (kept as it was if omitted; first time: instruct)
  --sync / --no-sync            Let the end of an agent turn sync accepted pages with the Library (kept as it was if omitted)
  --dry-run                     Say what would change and change nothing
  --json                        Machine-readable output
`;

function flags(argv: string[]) {
  const out: { agents?: AgentId[]; athenaeum?: string; workspace?: string; cwd?: string; allow?: string[]; capture?: CaptureMode; sync?: boolean; dryRun?: boolean; json?: boolean; help?: boolean } = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]!;
    const [flag, inline] = arg.startsWith('--') && arg.includes('=') ? [arg.slice(0, arg.indexOf('=')), arg.slice(arg.indexOf('=') + 1)] : [arg, undefined];
    const value = (): string => {
      if (inline !== undefined) return inline;
      const next = argv[i + 1];
      if (next === undefined || next.startsWith('--')) throw new Error(`${flag} needs a value`);
      i += 1;
      return next;
    };
    switch (flag) {
      case '--agent':
      case '--agents': {
        const list = value().split(',').map((s) => s.trim()).filter(Boolean);
        for (const agent of list) if (!(AGENT_IDS as readonly string[]).includes(agent)) throw new Error(`unknown agent ${JSON.stringify(agent)}; use ${AGENT_IDS.join(', ')}`);
        out.agents = [...(out.agents ?? []), ...(list as AgentId[])];
        break;
      }
      case '--athenaeum': out.athenaeum = value(); break;
      case '--workspace': out.workspace = value(); break;
      case '--cwd': out.cwd = value(); break;
      case '--allow': out.allow = value().split(',').map((s) => s.trim()).filter(Boolean); break;
      case '--capture': {
        const mode = value();
        if (!(CAPTURE_MODES as readonly string[]).includes(mode)) throw new Error(`--capture must be one of ${CAPTURE_MODES.join(', ')}`);
        out.capture = mode as CaptureMode;
        break;
      }
      case '--sync': out.sync = true; break;
      case '--no-sync': out.sync = false; break;
      case '--dry-run': out.dryRun = true; break;
      case '--json': out.json = true; break;
      case '--help': case '-h': out.help = true; break;
      default: throw new Error(`unknown argument ${JSON.stringify(arg)}`);
    }
  }
  return out;
}

const LABEL: Record<AgentId, string> = { claude: 'Claude Code', athenaeum: 'Athenaeum Agent', cursor: 'Cursor' };
const MARK: Record<AgentReport['status'], string> = { connected: 'ok  ', unchanged: 'ok  ', removed: 'ok  ', not_connected: '-   ', skipped: '-   ', conflict: 'FAIL', error: 'FAIL' };

async function readStdin(): Promise<string> {
  if (process.stdin.isTTY) return '';
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) chunks.push(chunk as Buffer);
  return Buffer.concat(chunks).toString('utf8');
}

async function main(argv: string[]): Promise<number> {
  const [command, ...rest] = argv;
  switch (command) {
    case 'mcp': {
      const { StdioServerTransport } = await import('@modelcontextprotocol/sdk/server/stdio.js');
      const { createMcpServer } = await import('./mcp/index.ts');
      const args = parseArguments(rest);
      if (args.help) { process.stdout.write('Usage: athenaeum-integrations mcp [--repo NAME=PATH] [--allow propose,review,sync] [--config FILE] [--athenaeum COMMAND] [--caller NAME] [--session ID]\n'); return 0; }
      await createMcpServer(loadConfig(args)).connect(new StdioServerTransport());
      return -1; // the server keeps the process alive
    }
    case 'hook': {
      const [agent, event, ...options] = rest;
      // A hook that fails the agent is worse than no hook, so every problem here is silence.
      if (agent !== 'claude' || !HOOK_EVENTS.includes(event as HookEvent)) return 0;
      let athenaeum: string | undefined; let workspace: string | undefined; let capture: CaptureMode | undefined; let sync = false;
      for (let i = 0; i < options.length; i += 1) {
        if (options[i] === '--athenaeum') athenaeum = options[(i += 1)];
        else if (options[i] === '--workspace') workspace = options[(i += 1)];
        else if (options[i] === '--capture') capture = options[(i += 1)] as CaptureMode;
        else if (options[i] === '--sync') sync = true;
      }
      const result = await runHook({ event: event as HookEvent, stdin: await readStdin(), workspace, ...(athenaeum ? { athenaeum } : {}), ...(capture && CAPTURE_MODES.includes(capture) ? { capture } : {}), sync });
      process.stdout.write(result.stdout);
      return 0;
    }
    case 'connect':
    case 'disconnect':
    case 'doctor': {
      const f = flags(rest);
      if (f.help) { process.stdout.write(USAGE); return 0; }
      const options: ConnectOptions = { ...(f.workspace ? { workspace: f.workspace } : {}), ...(f.agents ? { agents: f.agents } : {}), ...(f.athenaeum ? { athenaeum: f.athenaeum } : {}), ...(f.allow ? { allow: f.allow } : {}), ...(f.capture ? { capture: f.capture } : {}), ...(f.sync !== undefined ? { sync: f.sync } : {}), ...(f.dryRun ? { dryRun: true } : {}) };
      if (command === 'doctor') {
        const diagnosis = await diagnose({ ...options, cwd: f.cwd });
        if (f.json) process.stdout.write(`${JSON.stringify(diagnosis, null, 2)}\n`);
        else {
          for (const c of diagnosis.checks) process.stdout.write(`${c.ok ? 'ok  ' : 'FAIL'} ${c.id}: ${c.detail}${c.fix ? `\n       fix: ${c.fix}` : ''}\n`);
          process.stdout.write(diagnosis.ok ? '\nEverything is connected.\n' : '\nSomething needs attention (see above).\n');
        }
        return diagnosis.ok ? 0 : 1;
      }
      const result = command === 'connect' ? connect(options) : disconnect(options);
      if (f.json) process.stdout.write(`${JSON.stringify(result, null, 2)}\n`);
      else {
        for (const a of result.agents) process.stdout.write(`${MARK[a.status]} ${LABEL[a.agent]}: ${a.detail}${a.detected || command !== 'connect' ? '' : ' (not found on this machine; configured anyway)'}\n`);
        if (result.note) process.stdout.write(`${result.note}\n`);
        if (options.dryRun) process.stdout.write('\nDry run: nothing was written.\n');
      }
      return result.ok ? 0 : 1;
    }
    case '--help':
    case '-h':
      process.stdout.write(USAGE);
      return 0;
    default: {
      const args = parseArguments(argv);
      if (args.help) process.stdout.write(USAGE);
      else process.stdout.write(`${JSON.stringify({ mcpServers: { athenaeum: mcpLaunch(loadConfig(args), 'athenaeum-mcp', args.file) } }, null, 2)}\n`);
      return 0;
    }
  }
}

main(process.argv.slice(2)).then(
  (code) => { if (code >= 0) process.exitCode = code; },
  (error) => { process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`); process.exitCode = 1; },
);
