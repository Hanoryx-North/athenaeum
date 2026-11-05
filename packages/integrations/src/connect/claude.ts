import { commandLine } from './shell.ts';
import { backupOnce, readJsonObject, removeIfOurs, writeJson } from './files.ts';
import { agentPaths, detected } from './paths.ts';
import { CAPTURE_MODES, type AgentReport, type CaptureMode, type ConnectContext } from './types.ts';
import { savedWorkspace, workspaceArgs } from './workspace.ts';

/** The three events Athenaeum listens to, with the seconds the agent may wait for each. */
export const CLAUDE_EVENTS = [
  { event: 'SessionStart', hook: 'session-start', timeout: 20 },
  { event: 'UserPromptSubmit', hook: 'user-prompt', timeout: 15 },
  { event: 'Stop', hook: 'stop', timeout: 90 },
] as const;

export function mcpEntry(ctx: ConnectContext, caller: string) {
  return {
    type: 'stdio',
    command: ctx.launch.node,
    args: [ctx.launch.script, 'mcp', '--athenaeum', ctx.athenaeum, ...workspaceArgs(ctx), '--capture', ctx.capture, ...(ctx.allow.length ? ['--allow', ctx.allow.join(',')] : []), '--caller', caller],
  };
}

function hookCommand(ctx: ConnectContext, hook: string): string {
  return commandLine(
    [ctx.launch.node, ctx.launch.script, 'hook', 'claude', hook, '--athenaeum', ctx.athenaeum, ...(ctx.workspace ? ['--workspace', ctx.workspace] : []), '--capture', ctx.capture, ...(hook === 'stop' && ctx.sync ? ['--sync'] : [])],
    ctx.platform,
  );
}

/** One of ours: a Claude hook group that runs `hook claude <event> --athenaeum …`. */
export function isManagedGroup(group: any): boolean {
  return (
    Array.isArray(group?.hooks) &&
    group.hooks.some((h: any) => typeof h?.command === 'string' && managedHook(h.command) !== undefined && h.command.includes('--athenaeum'))
  );
}

/** Windows quotes every argument; recognize the tokens in both supported shell formats. */
function managedHook(command: string): string | undefined {
  return /(?:^|\s)["']?hook["']?\s+["']?claude["']?\s+["']?(session-start|user-prompt|stop)["']?(?=\s|$)/.exec(command)?.[1];
}

/** The choices already written into our Stop hook, so a later connect can keep them. */
export function existingChoices(ctx: Pick<ConnectContext, 'env' | 'home'>): { capture?: CaptureMode; sync?: boolean } {
  try {
    const hooks = readJsonObject(agentPaths(ctx.env, ctx.home).claude.settings).hooks?.Stop;
    for (const group of Array.isArray(hooks) ? hooks : []) {
      if (!isManagedGroup(group)) continue;
      for (const h of group.hooks) {
        if (typeof h?.command !== 'string' || managedHook(h.command) !== 'stop') continue;
        const mode = /["']?--capture["']?\s+["']?(\w+)/.exec(h.command)?.[1];
        return { ...(mode && (CAPTURE_MODES as readonly string[]).includes(mode) ? { capture: mode as CaptureMode } : {}), sync: /(^|\s)["']?--sync["']?(\s|$)/.test(h.command) };
      }
    }
  } catch {
    /* unreadable settings are reported by connect itself */
  }
  return {};
}

export function connectClaude(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).claude;
  const files = [paths.mcp, paths.settings];
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'claude', detected: detected('claude', ctx), status, files, detail });
  try {
    const mcp = readJsonObject(paths.mcp);
    ctx = { ...ctx, workspace: ctx.workspace ?? savedWorkspace(mcp.mcpServers?.athenaeum?.args) };
    const settings = readJsonObject(paths.settings);
    const before = JSON.stringify([mcp.mcpServers?.athenaeum, settings.hooks]);

    mcp.mcpServers = { ...(mcp.mcpServers ?? {}), athenaeum: mcpEntry(ctx, 'claude-code') };
    settings.hooks = { ...(settings.hooks ?? {}) };
    for (const { event, hook, timeout } of CLAUDE_EVENTS) {
      const kept = (Array.isArray(settings.hooks[event]) ? settings.hooks[event] : []).filter((g: any) => !isManagedGroup(g));
      settings.hooks[event] = [...kept, { hooks: [{ type: 'command', command: hookCommand(ctx, hook), timeout }] }];
    }
    const after = JSON.stringify([mcp.mcpServers.athenaeum, settings.hooks]);
    if (before === after) return report('unchanged', 'Claude Code already uses this Athenaeum.');
    if (!ctx.dryRun) {
      backupOnce(paths.mcp);
      backupOnce(paths.settings);
      writeJson(paths.mcp, mcp);
      writeJson(paths.settings, settings);
    }
    return report('connected', `Claude Code gets the athenaeum MCP server and ${CLAUDE_EVENTS.length} hooks (project recall at session start and on each prompt, upkeep when a turn ends).`);
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export function disconnectClaude(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).claude;
  const files = [paths.mcp, paths.settings];
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'claude', detected: detected('claude', ctx), status, files, detail });
  try {
    const mcp = readJsonObject(paths.mcp);
    const settings = readJsonObject(paths.settings);
    let changed = false;
    if (mcp.mcpServers && 'athenaeum' in mcp.mcpServers) {
      delete mcp.mcpServers.athenaeum;
      if (!Object.keys(mcp.mcpServers).length) delete mcp.mcpServers;
      changed = true;
    }
    if (settings.hooks && typeof settings.hooks === 'object') {
      for (const event of Object.keys(settings.hooks)) {
        const list = Array.isArray(settings.hooks[event]) ? settings.hooks[event] : [];
        const kept = list.filter((g: any) => !isManagedGroup(g));
        if (kept.length !== list.length) {
          changed = true;
          if (kept.length) settings.hooks[event] = kept;
          else delete settings.hooks[event];
        }
      }
      if (!Object.keys(settings.hooks).length) delete settings.hooks;
    }
    if (!changed) return report('not_connected', 'Nothing of Athenaeum\'s was in Claude Code\'s settings.');
    if (!ctx.dryRun) {
      writeJson(paths.mcp, mcp);
      writeJson(paths.settings, settings);
      removeIfOurs(paths.mcp);
      removeIfOurs(paths.settings);
    }
    return report('removed', 'Removed the athenaeum MCP server and hooks from Claude Code.');
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export interface Inspection {
  connected: boolean;
  problems: string[];
  workspace?: string;
}

export function inspectClaude(ctx: ConnectContext): Inspection {
  const paths = agentPaths(ctx.env, ctx.home).claude;
  const problems: string[] = [];
  let workspace: string | undefined;
  try {
    const entry = readJsonObject(paths.mcp).mcpServers?.athenaeum;
    workspace = savedWorkspace(entry?.args);
    if (!entry) problems.push('the athenaeum MCP server is not registered');
    const hooks = readJsonObject(paths.settings).hooks ?? {};
    for (const { event } of CLAUDE_EVENTS) {
      if (!(Array.isArray(hooks[event]) ? hooks[event] : []).some(isManagedGroup)) problems.push(`the ${event} hook is missing`);
    }
  } catch (error: any) {
    problems.push(error?.message ?? String(error));
  }
  return { connected: problems.length === 0, problems, workspace };
}
