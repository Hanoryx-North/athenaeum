import { beforeEach, expect, it, vi } from "vitest"

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue(null) }))
vi.mock("@tauri-apps/plugin-autostart", () => ({ isEnabled: vi.fn().mockResolvedValue(false), enable: vi.fn(), disable: vi.fn() }))
vi.mock("@/i18n", () => ({ default: { changeLanguage: vi.fn().mockResolvedValue(null) } }))
vi.mock("./project-store", () => Object.fromEntries([
  "loadLlmConfig", "loadProviderConfigs", "loadActivePresetId", "loadTaskModelRouting", "loadLanguage",
  "loadSearchApiConfig", "loadEmbeddingConfig", "loadMultimodalConfig", "loadMineruConfig", "loadProxyConfig", "loadApiConfig",
  "loadCustomLlmPresets", "loadGeneralConfig", "loadZoomLevel", "saveLlmConfig",
].map(name => [name, vi.fn().mockResolvedValue(null)])))
import * as saved from "./project-store"
import { hydrateApplicationSettings } from "./app-startup"
import { useWikiStore } from "@/stores/wiki-store"

beforeEach(() => {
  vi.mocked(saved.loadCustomLlmPresets).mockResolvedValue([])
  vi.mocked(saved.loadGeneralConfig).mockResolvedValue({ autostart: false, closeBehavior: "minimize" })
})

it("a damaged optional preference does not prevent the other settings or project startup", async () => {
  const warning = vi.spyOn(console, "warn").mockImplementation(() => {})
  const model = { ...useWikiStore.getState().llmConfig, model: "restored-model" }
  vi.mocked(saved.loadZoomLevel).mockRejectedValue(new Error("Damaged zoom preference"))
  vi.mocked(saved.loadLlmConfig).mockResolvedValue(model)
  await expect(hydrateApplicationSettings()).resolves.toBeUndefined()
  expect(useWikiStore.getState().llmConfig.model).toBe("restored-model")
  expect(saved.loadSearchApiConfig).toHaveBeenCalled()
  expect(warning).toHaveBeenCalledWith("Could not load zoom settings:", expect.any(Error))
  warning.mockRestore()
})
