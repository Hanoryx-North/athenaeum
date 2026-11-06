import { describe, expect, it } from "vitest"
import { nativeRouteFor } from "./harness-route"
import type { LlmConfig } from "@/stores/wiki-store"
const config = (patch: Partial<LlmConfig>): LlmConfig => ({ provider: "openai", apiKey: "example", model: "configured-model", ollamaUrl: "", customEndpoint: "", maxContextSize: 131072, ...patch })
describe("owned agent model routing", () => {
  it.each([
    ["openai", "responses", "https://api.openai.com/v1"],
    ["anthropic", "anthropic", "https://api.anthropic.com/v1"],
    ["ollama", "chat-completions", "http://localhost:11434/v1"],
  ] as const)("routes %s through its actual API", (provider, api, baseURL) => {
    expect(nativeRouteFor(config({ provider }))).toMatchObject({ ok: true, route: { provider: "athenaeum-native", model: "configured-model", piProvider: { api, baseURL } } })
  })
  it("accepts a keyless custom local provider and removes a pasted API suffix", () => {
    expect(nativeRouteFor(config({ provider: "custom", apiKey: "", customEndpoint: "http://127.0.0.1:1234/v1/chat/completions" }))).toMatchObject({ ok: true, route: { piProvider: { baseURL: "http://127.0.0.1:1234/v1", api: "chat-completions" } } })
  })
  it("keeps Anthropic-compatible custom routes on Messages", () => {
    expect(nativeRouteFor(config({ provider: "custom", customEndpoint: "https://provider.example/v1", apiMode: "anthropic_messages" }))).toMatchObject({ ok: true, route: { piProvider: { api: "anthropic" } } })
  })
  it("reports unsupported providers rather than silently ignoring Settings", () => {
    expect(nativeRouteFor(config({ provider: "google" })).ok).toBe(false)
    expect(nativeRouteFor(config({ provider: "azure" })).ok).toBe(false)
  })
  it("requires a model and credentials for hosted providers", () => {
    expect(nativeRouteFor(config({ model: "" })).ok).toBe(false)
    expect(nativeRouteFor(config({ apiKey: "" })).ok).toBe(false)
  })
})
