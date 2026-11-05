import { backupOnce, readJsonObject, removeIfOurs, writeJson } from './files.ts';
import { agentPaths, detected } from './paths.ts';
import { mcpEntry } from './claude.ts';
import type { AgentReport, ConnectContext } from './types.ts';
import { savedWorkspace } from './workspace.ts';

export function connectCursor(ctx: ConnectContext): AgentReport {
  const file = agentPaths(ctx.env, ctx.home).cursor.mcp;
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'cursor', detected: detected('cursor', ctx), status, files: [file], detail });
  try {
    const config = readJsonObject(file);
    ctx = { ...ctx, workspace: ctx.workspace ?? savedWorkspace(config.mcpServers?.athenaeum?.args) };
    const entry = mcpEntry(ctx, 'cursor');
    if (JSON.stringify(config.mcpServers?.athenaeum) === JSON.stringify(entry)) return report('unchanged', 'Cursor already uses this Athenaeum.');
    config.mcpServers = { ...(config.mcpServers ?? {}), athenaeum: entry };
    if (!ctx.dryRun) {
      backupOnce(file);
      writeJson(file, config);
    }
    return report('connected', 'Cursor gets the athenaeum MCP server. Cursor has no session hooks, so it recalls only when it asks.');
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export function disconnectCursor(ctx: ConnectContext): AgentReport {
  const file = agentPaths(ctx.env, ctx.home).cursor.mcp;
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'cursor', detected: detected('cursor', ctx), status, files: [file], detail });
  try {
    const config = readJsonObject(file);
    if (!config.mcpServers || !('athenaeum' in config.mcpServers)) return report('not_connected', 'Nothing of Athenaeum\'s was in Cursor\'s MCP settings.');
    delete config.mcpServers.athenaeum;
    if (!Object.keys(config.mcpServers).length) delete config.mcpServers;
    if (!ctx.dryRun) {
      writeJson(file, config);
      removeIfOurs(file);
    }
    return report('removed', 'Removed the athenaeum MCP server from Cursor.');
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export function inspectCursor(ctx: ConnectContext): { connected: boolean; problems: string[]; workspace?: string } {
  try {
    const entry = readJsonObject(agentPaths(ctx.env, ctx.home).cursor.mcp).mcpServers?.athenaeum;
    const present = Boolean(entry);
    return { connected: present, problems: present ? [] : ['the athenaeum MCP server is not registered'], workspace: savedWorkspace(entry?.args) };
  } catch (error: any) {
    return { connected: false, problems: [error?.message ?? String(error)] };
  }
}
