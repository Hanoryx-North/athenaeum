import { create } from "zustand"
import { invoke } from "@tauri-apps/api/core"

export interface BackgroundMemorySettings {
  enabled: boolean
  includeCodex: boolean
  model: string
  wikiRoot?: string
}
interface MemoryJob {
  id: string
  status: string
  source?: string
  model?: string
  proposed?: string[]
  failureStage?: string
  outcome?: string
  extraction?: {
    completedChunks?: number
    totalChunks?: number
    coverage?: { truncated: boolean }
  }
}
interface MemoryStatus {
  settings: BackgroundMemorySettings
  state: "idle" | "running"
  jobs: MemoryJob[]
  error: string | null
  lastRunAt: string | null
  active?: boolean
}
interface BackgroundMemoryState {
  status: MemoryStatus | null
  busy: boolean
  error: string | null
  refresh: (workspace: string) => Promise<void>
  configure: (workspace: string, settings: BackgroundMemorySettings) => Promise<void>
  run: (workspace: string) => Promise<void>
}
let generation = 0
export const useBackgroundMemoryStore = create<BackgroundMemoryState>((set, get) => {
  const request = async (workspace: string, action: string, settings?: BackgroundMemorySettings) => {
    if (get().busy) return
    const version = ++generation
    set({ busy: true, error: null })
    try {
      const status = await invoke<MemoryStatus>("background_memory", { projectPath: workspace, action,
        ...(settings ? { settings: { ...settings, wikiRoot: workspace } } : {}) })
      if (version === generation) set({ status })
    } catch (error) {
      if (version === generation) set({ error: error instanceof Error ? error.message : String(error) })
    } finally { if (version === generation) set({ busy: false }) }
  }
  return {
    status: null, busy: false, error: null,
    refresh: (workspace) => request(workspace, "status"),
    configure: (workspace, settings) => request(workspace, "configure", settings),
    run: (workspace) => request(workspace, "run"),
  }
})
