import fs from 'node:fs';
import { backupOnce, hasManagedBlock, removeIfOurs, setManagedBlock, writeAtomic } from './files.ts';
import { agentPaths, detected } from './paths.ts';
import { CODEX_BLOCK } from './instructions.ts';
import type { AgentReport, CaptureMode, ConnectContext } from './types.ts';
import { workspaceArgs } from './workspace.ts';

const TOML_BEGIN = '# >>> athenaeum >>> managed by `athenaeum setup`; changes inside this block are overwritten';
const TOML_END = '# <<< athenaeum <<<';
const MD_BEGIN = '<!-- athenaeum:begin (managed by `athenaeum setup`) -->';
const MD_END = '<!-- athenaeum:end -->';

/** TOML basic strings share JSON's escapes for everything this package writes. */
const str = (value: string) => JSON.stringify(value);

function tomlBlock(ctx: ConnectContext): string {
  const args = [ctx.launch.script, 'mcp', '--athenaeum', ctx.athenaeum, ...workspaceArgs(ctx), '--capture', ctx.capture, ...(ctx.allow.length ? ['--allow', ctx.allow.join(',')] : []), '--caller', 'codex'];
  return [
    `# athenaeum-capture: ${ctx.capture}`,
    ...(ctx.workspace ? [`# athenaeum-workspace: ${str(ctx.workspace)}`] : []),
    '[mcp_servers.athenaeum]',
    `command = ${str(ctx.launch.node)}`,
    `args = [${args.map(str).join(', ')}]`,
    'startup_timeout_sec = 20',
    'tool_timeout_sec = 120',
  ].join('\n');
}

function read(file: string): string {
  try {
    return fs.readFileSync(file, 'utf8');
  } catch (error: any) {
    if (error?.code === 'ENOENT') return '';
    throw error;
  }
}

/** Codex loads a non-empty global override instead of the normal AGENTS.md. */
function instructionFiles(ctx: Pick<ConnectContext, 'env' | 'home'>) {
  const paths = agentPaths(ctx.env, ctx.home).codex;
  const override = read(paths.override);
  return { active: override.trim() ? paths.override : paths.instructions, files: [paths.instructions, paths.override] };
}

export function existingCodexCapture(ctx: Pick<ConnectContext, 'env' | 'home'>): CaptureMode | undefined {
  const config = read(agentPaths(ctx.env, ctx.home).codex.config);
  const start = config.indexOf(TOML_BEGIN);
  const stop = config.indexOf(TOML_END, start);
  if (start === -1 || stop === -1) return undefined;
  return config.slice(start, stop).match(/^# athenaeum-capture: (off|instruct|nudge)$/m)?.[1] as CaptureMode | undefined;
}

export function existingCodexWorkspace(ctx: Pick<ConnectContext, 'env' | 'home'>): string | undefined {
  const config = read(agentPaths(ctx.env, ctx.home).codex.config);
  const start = config.indexOf(TOML_BEGIN);
  const stop = config.indexOf(TOML_END, start);
  if (start < 0 || stop < 0) return undefined;
  const value = config.slice(start, stop).match(/^# athenaeum-workspace: (.+)$/m)?.[1];
  if (!value) return undefined;
  const parsed: unknown = JSON.parse(value);
  if (typeof parsed !== 'string') throw new Error('Invalid saved Athenaeum workspace');
  return parsed;
}

/** The user's own `[mcp_servers.athenaeum]`, outside our block. We never edit or shadow it. */
function foreignEntry(text: string): boolean {
  const start = text.indexOf(TOML_BEGIN);
  const stop = start === -1 ? -1 : text.indexOf(TOML_END, start);
  const outside = start !== -1 && stop !== -1 ? text.slice(0, start) + text.slice(stop + TOML_END.length) : text;
  return /^\s*\[\s*mcp_servers\s*\.\s*"?athenaeum"?\s*\]/m.test(outside);
}

export function connectCodex(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).codex;
  const files = [paths.config];
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'codex', detected: detected('codex', ctx), status, files, detail });
  try {
    if (ctx.preserveCapture) ctx = { ...ctx, capture: existingCodexCapture(ctx) ?? ctx.capture };
    ctx = { ...ctx, workspace: ctx.workspace ?? existingCodexWorkspace(ctx) };
    const config = read(paths.config);
    if (foreignEntry(config)) {
      return report('conflict', `${paths.config} already defines [mcp_servers.athenaeum] by hand. Remove it (or rename it) and run this again; it was left untouched.`);
    }
    const nextConfig = setManagedBlock(config, TOML_BEGIN, TOML_END, tomlBlock(ctx));
    const instructions = instructionFiles(ctx);
    const edits = instructions.files.map((file) => {
      const before = read(file);
      // Refuse an incomplete block before writing any settings.
      if ((before.includes(MD_BEGIN) || before.includes(MD_END)) && !hasManagedBlock(before, MD_BEGIN, MD_END)) {
        throw new Error(`${file} has an incomplete Athenaeum instruction block. Repair the markers and retry setup.`);
      }
      const after = setManagedBlock(before, MD_BEGIN, MD_END, ctx.capture !== 'off' && file === instructions.active ? CODEX_BLOCK : null);
      return { file, before, after };
    });
    files.push(instructions.active);
    if (nextConfig === config && edits.every(({ before, after }) => before === after)) return report('unchanged', 'Codex already has the wiki tools and memory instructions.');
    if (!ctx.dryRun) {
      if (nextConfig !== config) {
        backupOnce(paths.config);
        writeAtomic(paths.config, nextConfig);
      }
      for (const { file, before, after } of edits) {
        if (before === after) continue;
        backupOnce(file);
        writeAtomic(file, after);
        removeIfOurs(file);
      }
    }
    return report('connected', ctx.capture === 'off'
      ? 'Codex has the wiki tools. Automatic memory instructions are turned off.'
      : 'Codex has the wiki tools and AGENTS.md memory instructions: recall before each task and propose durable findings before finishing, in the existing session. No additional API key or agent runtime. Reopen Codex to load the instructions.');
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export function disconnectCodex(ctx: ConnectContext): AgentReport {
  const paths = agentPaths(ctx.env, ctx.home).codex;
  const files = [paths.config, paths.instructions, paths.override];
  const report = (status: AgentReport['status'], detail: string): AgentReport => ({ agent: 'codex', detected: detected('codex', ctx), status, files, detail });
  try {
    const config = read(paths.config);
    const instructions = [paths.instructions, paths.override].map((file) => ({ file, text: read(file) }));
    const had = hasManagedBlock(config, TOML_BEGIN, TOML_END) || instructions.some(({ text }) => hasManagedBlock(text, MD_BEGIN, MD_END));
    if (!had) return report('not_connected', 'Nothing of Athenaeum\'s was in Codex\'s settings.');
    if (!ctx.dryRun) {
      if (hasManagedBlock(config, TOML_BEGIN, TOML_END)) writeAtomic(paths.config, setManagedBlock(config, TOML_BEGIN, TOML_END, null));
      for (const { file, text } of instructions) {
        if (hasManagedBlock(text, MD_BEGIN, MD_END)) writeAtomic(file, setManagedBlock(text, MD_BEGIN, MD_END, null));
        removeIfOurs(file);
      }
      removeIfOurs(paths.config);
    }
    return report('removed', 'Removed the athenaeum MCP server and instructions from Codex.');
  } catch (error: any) {
    return report('error', error?.message ?? String(error));
  }
}

export function inspectCodex(ctx: ConnectContext): { connected: boolean; problems: string[]; workspace?: string } {
  const paths = agentPaths(ctx.env, ctx.home).codex;
  try {
    const present = hasManagedBlock(read(paths.config), TOML_BEGIN, TOML_END);
    const problems = present ? [] : ['the athenaeum MCP server is not registered'];
    if (present && existingCodexCapture(ctx) !== 'off') {
      const active = instructionFiles(ctx).active;
      const text = read(active);
      if (!text.includes(`${MD_BEGIN}\n${CODEX_BLOCK}\n${MD_END}`)) problems.push(`wiki memory instructions are missing or outdated in ${active}`);
    }
    return { connected: problems.length === 0, problems, workspace: existingCodexWorkspace(ctx) };
  } catch (error: any) {
    return { connected: false, problems: [error?.message ?? String(error)] };
  }
}
