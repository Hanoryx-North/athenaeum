import { invoke } from "@tauri-apps/api/core"
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart"
import i18n from "@/i18n"
import { useWikiStore } from "@/stores/wiki-store"
import { useZoomStore } from "@/stores/zoom-store"
import * as saved from "./project-store"

async function hydrateModels() {
  const [config, providers, presets, active, routing] = await Promise.all([
    saved.loadLlmConfig(), saved.loadProviderConfigs(), saved.loadCustomLlmPresets(),
    saved.loadActivePresetId(), saved.loadTaskModelRouting(),
  ])
  const store = useWikiStore.getState()
  if (config) { store.setLlmConfig(config); store.setGlobalLlmConfig(config) }
  if (providers) store.setProviderConfigs(providers)
  store.setCustomLlmPresets(presets)
  if (routing) store.setTaskModelRouting(routing)
  if (!active) return
  store.setActivePresetId(active)
  const { findLlmPreset } = await import("@/components/settings/llm-presets")
  const { resolveConfig } = await import("@/components/settings/preset-resolver")
  const preset = findLlmPreset(active, presets)
  if (!preset) return
  const resolved = resolveConfig(preset, (providers ?? {})[active], useWikiStore.getState().llmConfig)
  store.setLlmConfig(resolved)
  store.setGlobalLlmConfig(resolved)
  await saved.saveLlmConfig(resolved)
}

async function hydrateGeneral() {
  const config = await saved.loadGeneralConfig()
  useWikiStore.getState().setGeneralConfig(config)
  const results = await Promise.allSettled([
    invoke("set_close_behavior", { value: config.closeBehavior }),
    (async () => {
      if (await isEnabled() !== config.autostart) await (config.autostart ? enable() : disable())
    })(),
  ])
  for (const result of results) if (result.status === "rejected") console.warn("Could not apply startup preference:", result.reason)
}

/** A damaged optional preference must not prevent project restoration. */
export async function hydrateApplicationSettings(): Promise<void> {
  const store = useWikiStore.getState()
  const tasks: [string, Promise<unknown>][] = [
    ["models", hydrateModels()],
    ["general", hydrateGeneral()],
    ["zoom", saved.loadZoomLevel().then(level => useZoomStore.getState().setLevel(level))],
    ["language", saved.loadLanguage().then(async language => { if (language) await i18n.changeLanguage(language) })],
    ["search", saved.loadSearchApiConfig().then(config => config && store.setSearchApiConfig(config))],
    ["embedding", saved.loadEmbeddingConfig().then(config => config && store.setEmbeddingConfig(config))],
    ["multimodal", saved.loadMultimodalConfig().then(config => config && store.setMultimodalConfig(config))],
    ["documents", saved.loadMineruConfig().then(config => config && store.setMineruConfig(config))],
    ["proxy", saved.loadProxyConfig().then(config => config && store.setProxyConfig(config))],
    ["api", saved.loadApiConfig().then(config => config && store.setApiConfig(config))],
  ]
  const results = await Promise.allSettled(tasks.map(([, task]) => task))
  results.forEach((result, index) => {
    if (result.status === "rejected") console.warn(`Could not load ${tasks[index][0]} settings:`, result.reason)
  })
}
