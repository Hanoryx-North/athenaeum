import { create } from "zustand"
import { useWikiStore } from "./wiki-store"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

export type ConnectorPage = "agents" | "account" | "model"

export const useConnectorsStore = create<{
  page: ConnectorPage
  setPage: (page: ConnectorPage) => void
}>((set) => ({
  page: (() => { const page = readUiPreference<ConnectorPage>("connector-page", "agents"); return ["agents", "account", "model"].includes(page) ? page : "agents" })(),
  setPage: (page) => { saveUiPreference("connector-page", page); set({ page }) },
}))

export function openConnectors(page?: ConnectorPage) {
  if (page) useConnectorsStore.getState().setPage(page)
  useWikiStore.getState().setActiveView("connectors")
}
