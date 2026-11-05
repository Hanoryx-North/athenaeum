export const AGENT_IDS = ['claude', 'codex', 'cursor'] as const;
export type AgentId = (typeof AGENT_IDS)[number];

export type CaptureMode = 'off' | 'instruct' | 'nudge';
export const CAPTURE_MODES: readonly CaptureMode[] = ['off', 'instruct', 'nudge'];

/** How to run this program again: the Node that is running it and the script it was started from. */
export interface Launch {
  node: string;
  script: string;
}

export type AgentStatus = 'connected' | 'unchanged' | 'removed' | 'not_connected' | 'skipped' | 'conflict' | 'error';

export interface AgentReport {
  agent: AgentId;
  /** Whether the agent looks installed on this machine. */
  detected: boolean;
  status: AgentStatus;
  /** Files written (or that would be, on a dry run). */
  files: string[];
  detail: string;
}

/** Everything a writer needs; built once so the three agents cannot disagree about it. */
export interface ConnectContext {
  home: string;
  env: NodeJS.ProcessEnv;
  platform: NodeJS.Platform;
  launch: Launch;
  /** Absolute path of the `athenaeum` command every hook and the MCP server will use. */
  athenaeum: string;
  /** Explicit wiki selected when connecting; independent of an agent's cwd. */
  workspace?: string;
  allow: string[];
  capture: CaptureMode;
  /** No explicit capture flag: keep each agent's saved choice when reconnecting. */
  preserveCapture?: boolean;
  sync: boolean;
  dryRun: boolean;
}
