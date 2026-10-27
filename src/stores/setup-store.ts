/**
 * Setup state: is the command-line tool installed, are the coding agents connected, is the project
 * linked to the Library. One store so the banner, the Library tab and the setup card agree.
 *
 * Nothing here runs by itself. A check runs when a project opens (it only reads), and everything
 * that changes something — connecting agents, signing in, syncing — runs on a click.
 */
import { create } from "zustand"
import {
  cancelCli,
  cliStatus,
  installTools,
  onCliProgress,
  parseDoctor,
  parseSetup,
  runCli,
  type CliJob,
  type CliStatus,
  type DoctorReport,
  type SetupReport,
  type JobResult,
} from "@/lib/cli-tools"

export type Busy = "check" | "install" | "setup" | "sync" | null

export interface DeviceCode {
  url: string
  code: string
}

interface SetupState {
  agentRoute: "external" | "athenaeum"
  cli: CliStatus | null
  doctor: DoctorReport | null
  lastSetup: SetupReport | null
  /** Where the project stands against the Library, as the CLI last said. */
  syncPosition: string | null
  syncMessage: string | null
  busy: Busy
  progress: string[]
  deviceCode: DeviceCode | null
  error: string | null
  check: (workspace: string) => Promise<void>
  install: (workspace: string | null, agent?: boolean) => Promise<boolean>
  setup: (workspace: string, job: Extract<CliJob, { job: "setup" }>) => Promise<boolean>
  sync: (workspace: string) => Promise<void>
  syncStatus: (workspace: string) => Promise<void>
  cancel: () => Promise<void>
  reset: () => Promise<void>
  operate: (workspace: string, job: CliJob) => Promise<JobResult | null>
}

const MAX_PROGRESS = 200
let setupGeneration = 0

function scopedSet(write: (partial: Partial<SetupState>) => void, generation: number) {
  return (partial: Partial<SetupState>) => { if (generation === setupGeneration) write(partial) }
}

function message(cause: unknown): string {
  const text = cause instanceof Error ? cause.message : String(cause)
  // The Rust side prefixes its errors with a stable code: "cli_missing: …".
  return text.replace(/^[a-z_]+: /, "")
}

/** Collect the CLI's progress while `work` runs, and surface the sign-in code the moment it arrives. */
async function withProgress<T>(
  set: (partial: Partial<SetupState>) => void,
  get: () => SetupState,
  work: () => Promise<T>,
): Promise<T> {
  set({ progress: [], deviceCode: null, error: null })
  const stop = await onCliProgress((p) => {
    if (p.event?.event === "device_code" && typeof p.event.url === "string" && typeof p.event.code === "string") {
      set({ deviceCode: { url: p.event.url, code: p.event.code } })
      return
    }
    set({ progress: [...get().progress, p.line].slice(-MAX_PROGRESS) })
  })
  try {
    return await work()
  } finally {
    stop()
    set({ deviceCode: null })
  }
}

export const useSetupStore = create<SetupState>((write, get) => ({
  agentRoute: "external",
  cli: null,
  doctor: null,
  lastSetup: null,
  syncPosition: null,
  syncMessage: null,
  busy: null,
  progress: [],
  deviceCode: null,
  error: null,

  check: async (workspace) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return
    set({ busy: "check", error: null })
    try {
      const cli = await cliStatus()
      set({ cli })
      if (!cli.found) {
        set({ doctor: null })
        return
      }
      const result = await runCli(workspace, { job: "doctor" })
      // The doctor exits non-zero when something needs attention; its report is the answer either way.
      set({ doctor: parseDoctor(result.output) })
    } catch (cause) {
      set({ error: message(cause) })
    } finally {
      set({ busy: null })
    }
  },

  install: async (workspace, agent = false) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return false
    set({ busy: "install" })
    let ok = false
    try {
      const result = await withProgress(set, get, () => installTools(agent))
      ok = result.exitCode === 0
      if (result.exitCode !== 0) {
        set({ error: result.stderr[result.stderr.length - 1] ?? "The installer did not finish." })
      }
      set({ cli: await cliStatus() })
    } catch (cause) {
      ok = false
      set({ error: message(cause) })
    } finally {
      set({ busy: null })
    }
    if (ok && generation === setupGeneration && workspace) await get().check(workspace)
    return ok
  },

  setup: async (workspace, job) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return false
    set({ busy: "setup" })
    let ok = false
    try {
      const result = await withProgress(set, get, () => runCli(workspace, job))
      const report = parseSetup(result.output)
      ok = result.exitCode === 0 && report?.ok === true
      set({ lastSetup: report })
      if (!report) set({ error: result.stderr[result.stderr.length - 1] ?? "Setup gave no report." })
    } catch (cause) {
      set({ error: message(cause) })
    } finally {
      set({ busy: null })
    }
    if (ok && generation === setupGeneration) await get().check(workspace)
    return ok
  },

  syncStatus: async (workspace) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return
    set({ busy: "check" })
    try {
      const result = await runCli(workspace, { job: "sync_status" })
      const out = result.output as { position?: string; detail?: string | null; error?: { message?: string } } | null
      set({ syncPosition: out?.position ?? null, syncMessage: out?.error?.message ?? out?.detail ?? null })
    } catch (cause) {
      set({ error: message(cause) })
    } finally {
      set({ busy: null })
    }
  },

  sync: async (workspace) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return
    set({ busy: "sync" })
    try {
      const result = await withProgress(set, get, () => runCli(workspace, { job: "sync" }))
      const out = result.output as { status?: string; message?: string; error?: { message?: string } } | null
      set({ syncMessage: out?.error?.message ?? out?.message ?? (result.exitCode === 0 ? "Synced." : "Sync stopped; see the Library tab.") })
    } catch (cause) {
      set({ error: message(cause) })
    } finally {
      set({ busy: null })
    }
    if (generation === setupGeneration) await get().syncStatus(workspace)
  },

  reset: async () => {
    setupGeneration += 1
    if (get().busy) await cancelCli()
    write({ doctor: null, lastSetup: null, syncPosition: null, syncMessage: null, busy: null, progress: [], deviceCode: null, error: null })
  },

  cancel: async () => {
    await cancelCli()
  },

  operate: async (workspace, job) => {
    const generation = setupGeneration
    const set = scopedSet(write, generation)
    if (get().busy) return null
    set({ busy: "sync", syncMessage: null })
    try {
      const result = await withProgress(set, get, () => runCli(workspace, job))
      if (result.exitCode !== 0) {
        const out = result.output as { error?: { message?: string }; message?: string } | null
        set({ error: out?.error?.message ?? out?.message ?? result.stderr[result.stderr.length - 1] ?? "The operation stopped. Check the connection and retry." })
      }
      return generation === setupGeneration ? result : null
    } catch (cause) {
      set({ error: message(cause) })
      return null
    } finally {
      set({ busy: null })
    }
  },
}))
