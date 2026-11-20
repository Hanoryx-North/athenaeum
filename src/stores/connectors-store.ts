import { create } from "zustand"
import { useWikiStore } from "./wiki-store"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

export type ConnectorPage = "agents" | "model"

export const useConnectorsStore = create<{
  page: ConnectorPage
  setPage: (page: ConnectorPage) => void
}>((set) => ({
  page: readUiPreference<string>("connector-page", "agents") === "model" ? "model" : "agents",
  setPage: (page) => { saveUiPreference("connector-page", page); set({ page }) },
}))

export function openConnectors(page?: ConnectorPage) {
  if (page) useConnectorsStore.getState().setPage(page)
  useWikiStore.getState().setActiveView("connectors")
}
