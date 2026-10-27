/**
 * The command-line tool, as this window can use it.
 *
 * Connecting coding agents, signing in to the Library and syncing are the CLI's work: it holds the
 * Library client and the credentials, and this window makes no network request to a hosted service.
 * What the window does is run that CLI on an explicit click and show what it says. The set of jobs
 * is closed here exactly as it is in `src-tauri/src/cli_tools.rs`; there is no "run this command".
 */
import { invoke } from "@tauri-apps/api/core"
import { listen, type UnlistenFn } from "@tauri-apps/api/event"

export interface CliStatus {
  found: boolean
  path: string | null
  version: string | null
  agentInstalled: boolean
}

export type CliJob =
  | { job: "doctor" }
  | {
      job: "setup"
      /** Connect the Library. An empty string means the default server. */
      library?: string
      autoSync?: boolean
      agents?: string[]
      noAgents?: boolean
      capture?: "off" | "instruct" | "nudge"
    }
  | { job: "sync_status" }
  | { job: "sync" }
  | { job: "disconnect_agents"; agents: string[] }
  | { job: "library_login" | "library_logout"; server: string }
  | { job: "sync_link"; server: string; name: string; public: boolean }
  | { job: "sync_clone"; server: string; remote: string; destination: string }
  | { job: "sync_push" | "sync_pull" | "sync_unlink" }
  | { job: "sync_conflict"; path: string }
  | { job: "sync_resolve"; path: string; keep: "local" | "remote" | "both" }

export interface JobResult {
  exitCode: number | null
  output: unknown
  stderr: string[]
}

export interface CliProgress {
  job: string
  line: string
  /** A structured event the CLI wrote, such as the code to confirm a sign-in with. */
  event: { event: string; [key: string]: unknown } | null
}

export interface DoctorCheck {
  id: string
  ok: boolean
  detail: string
  fix?: string
}

export interface DoctorReport {
  ok: boolean
  checks: DoctorCheck[]
}

export function cliStatus(): Promise<CliStatus> {
  return invoke<CliStatus>("cli_status")
}

export function runCli(workspace: string, job: CliJob): Promise<JobResult> {
  return invoke<JobResult>("cli_run", {
    workspace,
    // Rust's serde names are snake_case; the closed enum is the contract.
    job: job.job === "setup"
      ? { job: "setup", library: job.library, auto_sync: job.autoSync, agents: job.agents ?? [], no_agents: job.noAgents ?? false, capture: job.capture }
      : job,
  })
}

export function cancelCli(): Promise<boolean> {
  return invoke<boolean>("cli_cancel")
}

export function installTools(agent = false): Promise<JobResult> {
  return invoke<JobResult>("cli_install_tools", { agent })
}

export function onCliProgress(handler: (progress: CliProgress) => void): Promise<UnlistenFn> {
  return listen<CliProgress>("cli-progress", (event) => handler(event.payload))
}

/** The doctor's answer, checked rather than trusted: anything else is "no report". */
export function parseDoctor(output: unknown): DoctorReport | null {
  if (!output || typeof output !== "object") return null
  const { ok, checks } = output as { ok?: unknown; checks?: unknown }
  if (typeof ok !== "boolean" || !Array.isArray(checks)) return null
  const parsed: DoctorCheck[] = []
  for (const check of checks) {
    if (!check || typeof check !== "object") return null
    const { id, ok: checkOk, detail, fix } = check as Record<string, unknown>
    if (typeof id !== "string" || typeof checkOk !== "boolean" || typeof detail !== "string") return null
    parsed.push({ id, ok: checkOk, detail, ...(typeof fix === "string" ? { fix } : {}) })
  }
  return { ok, checks: parsed }
}

export interface SetupStep {
  id: string
  status: string
  detail: string
  fix?: string
}

export interface SetupReport {
  ok: boolean
  steps: SetupStep[]
  next: string[]
}

export function parseSetup(output: unknown): SetupReport | null {
  if (!output || typeof output !== "object") return null
  const { ok, steps, next } = output as { ok?: unknown; steps?: unknown; next?: unknown }
  if (typeof ok !== "boolean" || !Array.isArray(steps)) return null
  const parsed: SetupStep[] = []
  for (const step of steps) {
    if (!step || typeof step !== "object") return null
    const { id, status, detail, fix } = step as Record<string, unknown>
    if (typeof id !== "string" || typeof status !== "string" || typeof detail !== "string") return null
    parsed.push({ id, status, detail, ...(typeof fix === "string" ? { fix } : {}) })
  }
  return { ok, steps: parsed, next: Array.isArray(next) ? next.filter((n): n is string => typeof n === "string") : [] }
}
