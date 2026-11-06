import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { AgentId, ConnectContext } from './types.ts';

export function homeDir(env: NodeJS.ProcessEnv): string {
  return env.HOME || env.USERPROFILE || os.homedir();
}

/** Where each agent keeps the files this package edits. Environment overrides match the agents' own. */
export function agentPaths(env: NodeJS.ProcessEnv, home: string) {
  const claudeDir = env.CLAUDE_CONFIG_DIR || path.join(home, '.claude');
  const agentDir = env.ATHENAEUM_AGENT_HOME || path.join(home, '.athenaeum-agent');
  return {
    claude: {
      mcp: env.CLAUDE_CONFIG_DIR ? path.join(env.CLAUDE_CONFIG_DIR, '.claude.json') : path.join(home, '.claude.json'),
      settings: path.join(claudeDir, 'settings.json'),
      dir: claudeDir,
    },
    athenaeum: { dir: agentDir, config: path.join(agentDir, 'config.json'), instructions: path.join(agentDir, 'AGENT.md') },
    cursor: { dir: path.join(home, '.cursor'), mcp: path.join(home, '.cursor', 'mcp.json') },
  };
}

const COMMANDS: Record<AgentId, string> = { claude: 'claude', athenaeum: 'athenaeum-agent', cursor: 'cursor' };

export function onPath(name: string, env: NodeJS.ProcessEnv, platform: NodeJS.Platform = process.platform): string | undefined {
  const exts = platform === 'win32' ? (env.PATHEXT || '.EXE;.CMD;.BAT').split(';').concat('') : [''];
  const key = platform === 'win32' ? (Object.keys(env).find((k) => k.toLowerCase() === 'path') ?? 'Path') : 'PATH';
  for (const dir of (env[key] || '').split(platform === 'win32' ? ';' : ':')) {
    if (!dir) continue;
    for (const ext of exts) {
      const candidate = path.join(dir, name + ext);
      try {
        if (fs.statSync(candidate).isFile()) return candidate;
      } catch {
        /* next */
      }
    }
  }
  return undefined;
}

/** An agent counts as present if its command is on PATH or its config directory exists. */
export function detected(agent: AgentId, ctx: Pick<ConnectContext, 'env' | 'home' | 'platform'>): boolean {
  const paths = agentPaths(ctx.env, ctx.home);
  const dir = agent === 'claude' ? paths.claude.dir : paths[agent].dir;
  if (onPath(COMMANDS[agent], ctx.env, ctx.platform)) return true;
  try {
    if (fs.statSync(dir).isDirectory()) return true;
  } catch {
    /* absent */
  }
  return agent === 'claude' && fs.existsSync(paths.claude.mcp);
}
