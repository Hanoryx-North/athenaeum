/**
 * Which knowledge tab is open.
 *
 * One small store rather than local state, because other views link into a
 * specific tab: the sources tree points at scan coverage, and the review queue
 * points at health. A link that lands on the wrong tab is a small lie about
 * what the app did.
 */
import { create } from "zustand"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

export type KnowledgeTab = "health" | "sources" | "brief" | "library"

interface KnowledgeViewState {
  tab: KnowledgeTab
  setTab: (tab: KnowledgeTab) => void
}

export const useKnowledgeViewStore = create<KnowledgeViewState>((set) => ({
  tab: (() => { const tab = readUiPreference<KnowledgeTab>("knowledge-tab", "health"); return ["health", "sources", "brief", "library"].includes(tab) ? tab : "health" })(),
  setTab: (tab) => { saveUiPreference("knowledge-tab", tab); set({ tab }) },
}))
