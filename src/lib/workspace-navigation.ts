import type { WikiState } from "@/stores/wiki-store"
import { normalizePath } from "@/lib/path-utils"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

type Navigation = Pick<WikiState, "activeView" | "selectedFile">
const VIEWS: WikiState["activeView"][] = ["chat", "wiki", "sources", "search", "graph", "lint", "review", "knowledge", "skills", "settings", "connectors"]

export function loadWorkspaceNavigation(path: string): Navigation {
  const workspace = normalizePath(path)
  const saved = readUiPreference<Partial<Navigation> | null>(`navigation:${workspace}`, null)
  const selected = saved?.selectedFile
  return {
    activeView: saved?.activeView && VIEWS.includes(saved.activeView) ? saved.activeView : "connectors",
    selectedFile: typeof selected === "string" && normalizePath(selected).startsWith(`${workspace}/`) && !selected.split(/[\\/]/).includes("..") ? selected : null,
  }
}

export function saveWorkspaceNavigation(path: string, state: Navigation): void {
  saveUiPreference(`navigation:${normalizePath(path)}`, { activeView: state.activeView, selectedFile: state.selectedFile })
}
