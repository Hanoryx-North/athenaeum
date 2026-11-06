import fs from 'node:fs';
import { backupOnce, hasManagedBlock, removeIfOurs, setManagedBlock, writeAtomic } from './files.ts';
import { agentPaths, detected } from './paths.ts';
import { AGENT_BLOCK } from './instructions.ts';
import type { AgentReport, ConnectContext } from './types.ts';
import { workspaceArgs } from './workspace.ts';

const BEGIN = '<!-- athenaeum:begin (managed by `athenaeum setup`) -->';
const END = '<!-- athenaeum:end -->';
function read(file: string): string {
  try { return fs.readFileSync(file, 'utf8'); }
  catch (error: any) { if (error.code === 'ENOENT') return ''; throw error; }
}
function settings(ctx: ConnectContext) {
  const text = read(agentPaths(ctx.env, ctx.home).athenaeum.config);
  let value;
  try { value = text ? JSON.parse(text) : {}; }
  catch { throw new Error('Agent config.json contains invalid JSON; repair it before connecting.'); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Agent config.json must contain an object.');
  return value;
}
export function connectAthenaeum(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).athenaeum;
  const files = [paths.config, paths.instructions];
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'athenaeum', detected: detected('athenaeum', ctx), status, files, detail });
  try {
    const config = settings(ctx);
    const existing = config.mcpServers?.athenaeum;
    if (existing && existing.athenaeumManaged !== true) return report('conflict', 'Agent config.json already has a manually configured athenaeum MCP server; it was left untouched.');
    const capture = ctx.preserveCapture ? (existing?.capture ?? ctx.capture) : ctx.capture;
    const workspace = ctx.workspace ?? existing?.workspace;
    const before = read(paths.instructions);
    if ((before.includes(BEGIN) || before.includes(END)) && !hasManagedBlock(before, BEGIN, END)) throw new Error('Incomplete Athenaeum instruction block. Repair the markers and retry.');
    const after = setManagedBlock(before, BEGIN, END, capture === 'off' ? null : AGENT_BLOCK);
    const server = { athenaeumManaged: true, capture, ...(workspace ? { workspace } : {}),
      command: ctx.launch.node, args: [ctx.launch.script, 'mcp', '--athenaeum', ctx.athenaeum,
        ...workspaceArgs({ ...ctx, workspace }), '--capture', capture,
        ...(ctx.allow.length ? ['--allow', ctx.allow.join(',')] : []), '--caller', 'athenaeum-agent'] };
    const next = { ...config, mcpServers: { ...config.mcpServers, athenaeum: server } };
    if (JSON.stringify(next) === JSON.stringify(config) && before === after) return report('unchanged', 'Athenaeum Agent already has the wiki tools and memory instructions.');
    if (!ctx.dryRun) {
      backupOnce(paths.config); writeAtomic(paths.config, JSON.stringify(next, null, 2) + '\n');
      if (before !== after) { backupOnce(paths.instructions); writeAtomic(paths.instructions, after); removeIfOurs(paths.instructions); }
    }
    return report('connected', 'Athenaeum Agent is connected to the wiki. Choose a model in Settings for app chat, or configure the agent model in config.json.');
  } catch (error: any) { return report('error', error?.message ?? String(error)); }
}
export function disconnectAthenaeum(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).athenaeum;
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'athenaeum', detected: detected('athenaeum', ctx), status, files: [paths.config, paths.instructions], detail });
  try {
    const config = settings(ctx), before = read(paths.instructions);
    const owned = config.mcpServers?.athenaeum?.athenaeumManaged === true;
    if (!owned && !hasManagedBlock(before, BEGIN, END)) return report('not_connected', 'No managed wiki connection was found.');
    if (!ctx.dryRun) {
      if (owned) { delete config.mcpServers.athenaeum; if (!Object.keys(config.mcpServers).length) delete config.mcpServers; writeAtomic(paths.config, JSON.stringify(config, null, 2) + '\n'); }
      if (hasManagedBlock(before, BEGIN, END)) { writeAtomic(paths.instructions, setManagedBlock(before, BEGIN, END, null)); removeIfOurs(paths.instructions); }
    }
    return report('removed', 'Removed the managed wiki connection. Agent models and other settings were preserved.');
  } catch (error: any) { return report('error', error?.message ?? String(error)); }
}
export function inspectAthenaeum(ctx: ConnectContext): { connected: boolean; problems: string[]; workspace?: string } {
  try {
    const config = settings(ctx), server = config.mcpServers?.athenaeum;
    const problems = server?.athenaeumManaged === true ? [] : ['the wiki MCP server is not registered'];
    if (server?.athenaeumManaged && server.capture !== 'off' && !read(agentPaths(ctx.env, ctx.home).athenaeum.instructions).includes(`${BEGIN}\n${AGENT_BLOCK}\n${END}`)) problems.push('wiki memory instructions are missing or outdated');
    return { connected: problems.length === 0, problems, workspace: server?.workspace };
  } catch (error: any) { return { connected: false, problems: [error?.message ?? String(error)] }; }
}
