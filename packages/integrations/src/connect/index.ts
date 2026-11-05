import fs from 'node:fs';
import path from 'node:path';
import { run } from '../client/process.ts';
import { findWorkspace } from '../client/workspace.ts';
import { AthenaeumClient } from '../client/client.ts';
import { connectClaude, disconnectClaude, existingChoices, inspectClaude } from './claude.ts';
import { connectCodex, disconnectCodex, inspectCodex } from './codex.ts';
import { connectCursor, disconnectCursor, inspectCursor } from './cursor.ts';
import { detected, homeDir, onPath } from './paths.ts';
import { connectionWorkspace } from './workspace.ts';
import { AGENT_IDS, CAPTURE_MODES, type AgentId, type AgentReport, type CaptureMode, type ConnectContext, type Launch } from './types.ts';

export * from './types.ts';
export { INSTRUCTIONS, NUDGE } from './instructions.ts';

export interface ConnectOptions {
  /** Which agents to configure. `auto` (the default) configures the ones that look installed. */
  agents?: readonly AgentId[] | 'auto';
  /** The `athenaeum` command. Defaults to `ATHENAEUM_BIN`, the installer's `bin/`, then PATH. */
  athenaeum?: string;
  /** Bind the connection to this wiki, including sessions started elsewhere. */
  workspace?: string;
  /** MCP grants. Defaults to `propose`: an agent may queue knowledge, a person decides. */
  allow?: string[];
  /** How actively agents are asked to record knowledge. Left as it was when omitted (first time: `instruct`). */
  capture?: CaptureMode;
  /** Let the end of an agent turn sync accepted pages with the Library. Off unless asked for; left as it was when omitted. */
  sync?: boolean;
  dryRun?: boolean;
  env?: NodeJS.ProcessEnv;
  launch?: Launch;
  platform?: NodeJS.Platform;
  home?: string;
}

/** How to run this program again. The entry script is whatever started this process. */
export function selfLaunch(): Launch {
  const script = process.argv[1];
  if (!script) throw new Error('cannot tell which script is running; pass the launcher explicitly');
  return { node: fs.realpathSync.native(process.execPath), script: fs.realpathSync.native(script) };
}

/** The absolute path of the `athenaeum` command, or an error that says how to get one. */
export function resolveAthenaeum(options: Pick<ConnectOptions, 'athenaeum' | 'env' | 'platform' | 'launch'>): string {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const exe = platform === 'win32' ? 'athenaeum.exe' : 'athenaeum';
  const given = options.athenaeum || env.ATHENAEUM_BIN;
  const script = options.launch?.script ?? (process.argv[1] ? fs.realpathSync(process.argv[1]) : undefined);
  const beside = script ? [path.join(path.dirname(script), exe), path.join(path.dirname(script), '..', 'bin', exe)] : [];
  const candidates = given ? [given] : [...beside, onPath('athenaeum', env, platform)].filter((c): c is string => Boolean(c));
  for (const candidate of candidates) {
    const resolved = path.isAbsolute(candidate) ? candidate : (onPath(candidate, env, platform) ?? path.resolve(candidate));
    try {
      if (fs.statSync(resolved).isFile()) return fs.realpathSync.native(resolved);
    } catch {
      /* next */
    }
  }
  throw new Error(
    given
      ? `the athenaeum command ${JSON.stringify(given)} does not exist`
      : 'the `athenaeum` command was not found. Install Athenaeum (the installer puts it on your PATH), or pass --athenaeum /path/to/athenaeum.',
  );
}

export function buildContext(options: ConnectOptions): ConnectContext {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const launch = options.launch ?? selfLaunch();
  const home = options.home ?? homeDir(env);
  // Connecting again must not undo an earlier choice: only a flag changes what is already there.
  const before = existingChoices({ env, home });
  const capture = options.capture ?? before.capture ?? 'instruct';
  if (!CAPTURE_MODES.includes(capture)) throw new Error(`--capture must be one of ${CAPTURE_MODES.join(', ')}`);
  return {
    home,
    env,
    platform,
    launch,
    athenaeum: resolveAthenaeum({ ...options, env, platform, launch }),
    ...(options.workspace ? { workspace: connectionWorkspace(options.workspace) } : {}),
    allow: options.allow ?? ['propose'],
    capture,
    preserveCapture: options.capture === undefined,
    sync: options.sync ?? before.sync ?? false,
    dryRun: options.dryRun ?? false,
  };
}

function chosen(options: ConnectOptions, ctx: Pick<ConnectContext, 'env' | 'home' | 'platform'>): AgentId[] {
  if (Array.isArray(options.agents) && options.agents.length) return [...options.agents];
  return AGENT_IDS.filter((agent) => detected(agent, ctx));
}

const CONNECT = { claude: connectClaude, codex: connectCodex, cursor: connectCursor };
const DISCONNECT = { claude: disconnectClaude, codex: disconnectCodex, cursor: disconnectCursor };

export interface ConnectResult {
  ok: boolean;
  athenaeum: string;
  launcher: Launch;
  agents: AgentReport[];
  /** Why nothing was configured, when that is the case. */
  note?: string;
}

export function connect(options: ConnectOptions = {}): ConnectResult {
  const ctx = buildContext(options);
  const agents = chosen(options, ctx);
  if (!agents.length) {
    return { ok: true, athenaeum: ctx.athenaeum, launcher: ctx.launch, agents: [], note: 'No coding agent found (looked for Claude Code, Codex and Cursor). Install one, or name it with --agent.' };
  }
  const reports = agents.map((agent) => CONNECT[agent](ctx));
  return { ok: reports.every((r) => r.status !== 'error' && r.status !== 'conflict'), athenaeum: ctx.athenaeum, launcher: ctx.launch, agents: reports };
}

export function disconnect(options: ConnectOptions = {}): ConnectResult {
  // Removing needs no working CLI: fall back to a placeholder so a broken install can still be undone.
  let ctx: ConnectContext;
  try {
    ctx = buildContext(options);
  } catch {
    ctx = buildContext({ ...options, athenaeum: process.execPath });
  }
  const agents: AgentId[] = Array.isArray(options.agents) && options.agents.length ? [...options.agents] : [...AGENT_IDS];
  const reports = agents.map((agent) => DISCONNECT[agent](ctx));
  return { ok: reports.every((r) => r.status !== 'error'), athenaeum: ctx.athenaeum, launcher: ctx.launch, agents: reports };
}

export interface Check {
  id: string;
  ok: boolean;
  detail: string;
  /** The one thing to do about it, when it is not ok. */
  fix?: string;
}

export interface Diagnosis {
  ok: boolean;
  checks: Check[];
}

/** Every link between a coding agent and the knowledge base, checked from the outside. */
export async function diagnose(options: ConnectOptions & { cwd?: string } = {}): Promise<Diagnosis> {
  const env = options.env ?? process.env;
  const platform = options.platform ?? process.platform;
  const checks: Check[] = [];
  const add = (id: string, ok: boolean, detail: string, fix?: string) => checks.push({ id, ok, detail, ...(ok || !fix ? {} : { fix }) });

  let ctx: ConnectContext | undefined;
  try {
    ctx = buildContext({ ...options, env, platform });
    add('athenaeum', true, ctx.athenaeum);
  } catch (error: any) {
    add('athenaeum', false, error?.message ?? String(error), 'Install Athenaeum, or run the installer again.');
  }

  if (ctx) {
    try {
      const { stdout, exitCode } = await run(ctx.athenaeum, ['--version'], { timeoutMs: 10_000, maxBytes: 64 * 1024, env });
      add('athenaeum-runs', exitCode === 0, stdout.trim() || 'the command exited with an error', 'Reinstall Athenaeum.');
    } catch (error: any) {
      add('athenaeum-runs', false, error?.message ?? String(error), 'Reinstall Athenaeum.');
    }
    const inspections: Record<AgentId, () => { connected: boolean; problems: string[]; workspace?: string }> = {
      claude: () => inspectClaude(ctx!),
      codex: () => inspectCodex(ctx!),
      cursor: () => inspectCursor(ctx!),
    };
    for (const agent of chosen(options, ctx)) {
      const found = inspections[agent]();
      const expected = findWorkspace(options.cwd ?? process.cwd());
      const target = found.workspace ?? expected;
      if (found.connected) {
        if (!target) {
          found.problems.push('tools are registered, but there is no wiki in this folder and no wiki was selected');
        } else if (expected && path.resolve(target) !== path.resolve(expected)) {
          found.problems.push(`connected to ${target}, not the selected wiki ${expected}`);
        } else {
          try {
            const status = await new AthenaeumClient(target, { command: ctx.athenaeum, timeoutMs: 15_000, env }).status();
            // A new wiki can accept proposals before its first accepted Markdown page exists.
            if (!status.initialized) found.problems.push(`the selected wiki is unavailable: ${target}`);
            else if (status.indexStale) found.problems.push(`the selected wiki index is stale: ${target}; run \`wiki index-vault\``);
          } catch (error: any) {
            found.problems.push(error?.message ?? String(error));
          }
        }
      }
      add(`agent-${agent}`, found.problems.length === 0, found.problems.length ? found.problems.join('; ') : `memory ready: ${target}`, `Run \`athenaeum setup --agents ${agent}\` in the wiki you want to connect.`);
    }

    const workspace = findWorkspace(options.cwd ?? process.cwd());
    if (!workspace) {
      add('knowledge-base', true, 'no knowledge base in this directory (agents run normally here); `athenaeum setup` creates one');
    } else {
      try {
        const client = new AthenaeumClient(workspace, { command: ctx.athenaeum, timeoutMs: 15_000, env });
        const status = await client.status();
        add('knowledge-base', true, `${workspace}: ${status.nodes} nodes, ${status.proposals} proposals awaiting review`);
        const info = await client.repositoryInfo();
        add('library', true, info.remote ? `linked to ${info.remote.server}` : 'not linked to a Library (optional; `athenaeum setup --library`)');
      } catch (error: any) {
        add('knowledge-base', false, error?.message ?? String(error), 'Run `athenaeum index-vault`.');
      }
    }
  }
  return { ok: checks.every((c) => c.ok), checks };
}
