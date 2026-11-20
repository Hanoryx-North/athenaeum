import { create } from "zustand"
import { invoke } from "@tauri-apps/api/core"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"

export interface AutomationSettings { autoSync: boolean }
interface AutomationResult {
  settings: AutomationSettings
  batch: { accepted: unknown[]; errors: string[] }
  revision: number
}
interface AutomationState {
  workspace: string | null
  settings: AutomationSettings
  busy: boolean
  error: string | null
  revision: number | null
  run: (workspace: string, options?: { settings?: AutomationSettings }) => Promise<void>
}

export const useAutomationStore = create<AutomationState>((set, get) => ({
  workspace: null,
  settings: { autoSync: false },
  busy: false,
  error: null,
  revision: null,
  run: async (workspace, options = {}) => {
    if (get().busy) return
    const previousRevision = get().workspace === workspace ? get().revision : null
    set({ workspace, busy: true, error: null, ...(get().workspace !== workspace ? {
      settings: { autoSync: false }, revision: null,
    } : {}) })
    try {
      const result = await invoke<AutomationResult>("wiki_automation", {
        workspace, settings: options.settings,
      })
      if (useWikiStore.getState().project?.path !== workspace) return
      set({ settings: result.settings, revision: result.revision, error: result.batch.errors.join("\n") || null })
      if (result.batch.accepted.length || (previousRevision !== null && previousRevision !== result.revision)) {
        await refreshProjectFileTree(workspace, { bumpDataVersion: true })
        await useEngineStore.getState().refresh({ findings: false })
      }
    } catch (cause) {
      if (useWikiStore.getState().project?.path === workspace) set({ error: String(cause) })
    } finally { set({ busy: false }) }
  },
}))
