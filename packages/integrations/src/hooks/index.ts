import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { AthenaeumClient } from '../client/client.ts';
import { findWorkspace } from '../client/workspace.ts';
import { INSTRUCTIONS, NUDGE } from '../connect/instructions.ts';
import type { CaptureMode } from '../connect/types.ts';
import { resolveCommand } from '../client/config.ts';

export type HookEvent = 'session-start' | 'user-prompt' | 'stop';
export const HOOK_EVENTS: readonly HookEvent[] = ['session-start', 'user-prompt', 'stop'];

export interface HookOptions {
  event: HookEvent;
  /** The hook payload the agent wrote to stdin. */
  stdin: string;
  athenaeum?: string;
  workspace?: string;
  capture?: CaptureMode;
  sync?: boolean;
  env?: NodeJS.ProcessEnv;
  /** Where per-session memory is kept; defaults to the OS temp directory. */
  stateDir?: string;
}

/** What a hook prints. An empty string means "say nothing": the agent carries on as if we were not there. */
export interface HookResult {
  stdout: string;
}

const QUIET: HookResult = { stdout: '' };
const MIN_PROMPT = 16;
const MAX_CONTEXT = 8_000;
const EDIT_TOOLS = new Set(['Edit', 'Write', 'MultiEdit', 'NotebookEdit']);

interface Payload {
  cwd?: string;
  session_id?: string;
  prompt?: string;
  transcript_path?: string;
  stop_hook_active?: boolean;
}

interface SessionState {
  nudged?: boolean;
  lastSyncCheck?: number;
  emptyRecall?: { key: string; at: number };
}

/**
 * Run one hook. This never throws and never exits non-zero: a hook that fails must cost the person
 * nothing, so every failure path ends in silence. Set `ATHENAEUM_HOOK_DEBUG=1` to see why on stderr.
 */
export async function runHook(options: HookOptions): Promise<HookResult> {
  const env = options.env ?? process.env;
  try {
    return await dispatch(options, env);
  } catch (error: any) {
    if (env.ATHENAEUM_HOOK_DEBUG) process.stderr.write(`athenaeum hook ${options.event}: ${error?.stack ?? error}\n`);
    return QUIET;
  }
}

async function dispatch(options: HookOptions, env: NodeJS.ProcessEnv): Promise<HookResult> {
  let payload: Payload = {};
  try {
    const parsed = options.stdin.trim() ? JSON.parse(options.stdin) : {};
    if (parsed && typeof parsed === 'object') payload = parsed;
  } catch {
    return QUIET;
  }
  const workspace = options.workspace ?? findWorkspace(typeof payload.cwd === 'string' && payload.cwd ? payload.cwd : process.cwd());
  if (!workspace) return QUIET;

  const client = new AthenaeumClient(workspace, { command: options.athenaeum || resolveCommand(env), timeoutMs: 8_000, env });
  const capture = options.capture ?? 'instruct';
  switch (options.event) {
    case 'session-start':
      return sessionStart(client, capture);
    case 'user-prompt':
      return userPrompt(client, payload, options, env, workspace);
    case 'stop':
      return stop(client, payload, options, env, workspace);
  }
}

const output = (value: unknown): HookResult => ({ stdout: `${JSON.stringify(value)}\n` });

async function sessionStart(client: AthenaeumClient, capture: CaptureMode): Promise<HookResult> {
  const status = await client.status();
  if (!status.initialized) return QUIET;
  // Pick up pages edited outside Athenaeum (Obsidian, git pull, a person) before anything is recalled.
  if (status.indexStale || (status.hasVault && status.indexedAt == null)) await client.index({ scope: 'vault' }).catch(() => undefined);
  const lines = [`Athenaeum: this project's knowledge base has ${status.nodes} nodes${status.proposals ? ` and ${status.proposals} proposal${status.proposals === 1 ? '' : 's'} awaiting a person's review` : ''}.`];
  if (capture !== 'off') lines.push('', INSTRUCTIONS);
  lines.push('Task recall is handled automatically by the UserPromptSubmit hook. Do not repeat wiki_context for the same task, including when the hook supplies no knowledge.');
  return output({ hookSpecificOutput: { hookEventName: 'SessionStart', additionalContext: lines.join('\n') } });
}

async function userPrompt(client: AthenaeumClient, payload: Payload, options: HookOptions, env: NodeJS.ProcessEnv, workspace: string): Promise<HookResult> {
  const prompt = typeof payload.prompt === 'string' ? payload.prompt.trim() : '';
  // Slash commands and one-word replies carry no task to recall knowledge for.
  if (prompt.length < MIN_PROMPT || prompt.startsWith('/')) return QUIET;
  const state = new SessionMemory(options.stateDir ?? env.ATHENAEUM_HOOK_STATE_DIR ?? path.join(os.tmpdir(), 'athenaeum-hooks'), `${workspace}:${payload.session_id ?? ''}`);
  const memory = state.read();
  const key = crypto.createHash('sha256').update(prompt.slice(0, 2_000)).digest('hex');
  if (memory.emptyRecall?.key === key && Date.now() - memory.emptyRecall.at < 60_000) return QUIET;
  const context = await client.context({ task: prompt.slice(0, 2_000), depth: 1, maxNodes: 5 });
  if (!context.brief.nodes.length) {
    memory.emptyRecall = { key, at: Date.now() }; state.write(memory); return QUIET;
  }
  // Never cut off a trust wrapper or selectively remove its warnings.
  if (context.prompt.length > MAX_CONTEXT) return QUIET;
  const text = context.prompt;
  return output({ hookSpecificOutput: { hookEventName: 'UserPromptSubmit', additionalContext: text } });
}

async function stop(client: AthenaeumClient, payload: Payload, options: HookOptions, env: NodeJS.ProcessEnv, workspace: string): Promise<HookResult> {
  // The agent sets this when it is already continuing because of a stop hook. Acting again would
  // loop, so this is the one check that comes before everything else.
  if (payload.stop_hook_active) return QUIET;

  const state = new SessionMemory(options.stateDir ?? env.ATHENAEUM_HOOK_STATE_DIR ?? path.join(os.tmpdir(), 'athenaeum-hooks'), payload.session_id ?? workspace);
  const memory = state.read();

  const status = await client.status();
  if (!status.initialized) return QUIET;
  if (status.indexStale) await client.index({ scope: 'vault' }).catch(() => undefined);

  if (options.sync) {
    await maybeSync(client, memory);
  }

  let block: string | undefined;
  if ((options.capture ?? 'instruct') === 'nudge' && !memory.nudged && payload.transcript_path) {
    const seen = readTranscript(payload.transcript_path);
    if (seen.edited && !seen.proposed) {
      memory.nudged = true;
      block = NUDGE;
    }
  }
  state.write(memory);

  return block ? output({ decision: 'block', reason: block }) : QUIET;
}

const SYNC_CHECK_EVERY_MS = 60_000;
const SYNC_STATUS_BUDGET_MS = 8_000;
const SYNC_BUDGET_MS = 60_000;

const within = <T>(promise: Promise<T>, ms: number): Promise<T | undefined> =>
  Promise.race([promise, new Promise<undefined>((resolve) => setTimeout(() => resolve(undefined), ms).unref())]);

/**
 * Sync accepted wiki text, because the person asked for that when they connected (`--sync`). It
 * runs in the foreground of their own agent session, at most once a minute, only when the
 * repository is linked and out of step, and it stops at conflicts or a missing network instead of
 * forcing anything. It is not a background process.
 */
async function maybeSync(client: AthenaeumClient, memory: SessionState): Promise<void> {
  const now = Date.now();
  if (memory.lastSyncCheck && now - memory.lastSyncCheck < SYNC_CHECK_EVERY_MS) return undefined;
  memory.lastSyncCheck = now;
  const status = await within(client.sync('status').catch(() => undefined), SYNC_STATUS_BUDGET_MS);
  const position = (status?.result as { position?: string } | undefined)?.position;
  if (!status || !position || !['ahead', 'behind', 'diverged'].includes(position)) return undefined;
  await within(client.sync('sync', 'Sync from a coding session').catch(() => undefined), SYNC_BUDGET_MS);
}

/** What the transcript shows this session did. Reads only the tail, and only tool names. */
export function readTranscript(file: string): { edited: boolean; proposed: boolean } {
  const seen = { edited: false, proposed: false };
  let text: string;
  try {
    const fd = fs.openSync(file, 'r');
    try {
      const size = fs.fstatSync(fd).size;
      const length = Math.min(size, 4 * 1024 * 1024);
      const buffer = Buffer.alloc(length);
      fs.readSync(fd, buffer, 0, length, size - length);
      text = buffer.toString('utf8');
    } finally {
      fs.closeSync(fd);
    }
  } catch {
    return seen;
  }
  for (const line of text.split('\n')) {
    if (!line.includes('"tool_use"')) continue;
    try {
      const content = JSON.parse(line)?.message?.content;
      if (!Array.isArray(content)) continue;
      for (const part of content) {
        if (part?.type !== 'tool_use' || typeof part.name !== 'string') continue;
        if (EDIT_TOOLS.has(part.name)) seen.edited = true;
        if (/(^|__)wiki_(create|link|attach_evidence)$/.test(part.name)) seen.proposed = true;
      }
    } catch {
      /* a partial first line from the tail read, or a line that is not ours to parse */
    }
  }
  return seen;
}

class SessionMemory {
  private readonly file: string;
  constructor(dir: string, key: string) {
    this.file = path.join(dir, `${crypto.createHash('sha256').update(key).digest('hex').slice(0, 24)}.json`);
  }
  read(): SessionState {
    try {
      const value = JSON.parse(fs.readFileSync(this.file, 'utf8'));
      return value && typeof value === 'object' ? value : {};
    } catch {
      return {};
    }
  }
  write(state: SessionState): void {
    try {
      fs.mkdirSync(path.dirname(this.file), { recursive: true });
      fs.writeFileSync(this.file, JSON.stringify(state));
    } catch {
      /* losing this only means a message may repeat */
    }
  }
}
