import type { LlmConfig } from "@/stores/wiki-store"

/**
 * The chat agent's model route, derived from the provider chosen in Settings.
 *
 * Chat runs in DeepSeek Harness, which owns its adapters and reads credentials
 * from its environment. This only describes which of its routes to use; the
 * backend hands that to the runtime as a per-launch profile patch (naming the
 * key's variable, never the key) and puts the key in that variable of the
 * runtime's own process.
 */
export interface HarnessRoute {
  /** Harness provider route id, e.g. `openai` or `deepseek-official`. */
  provider: string
  model: string
  /** Environment variable the runtime reads the key from. */
  apiKeyEnv?: string
  apiKey?: string
  /** A `llm-pi-ai` provider profile to register under `provider`; absent for built-in routes. */
  piProvider?: Record<string, unknown>
}

export type HarnessRouteResult =
  | { ok: true; route: HarnessRoute }
  | { ok: false; reason: string }

/** The one variable a Settings key travels in, whichever route uses it. */
export const SETTINGS_KEY_ENV = "ATHENAEUM_LLM_API_KEY"
/** The route a custom endpoint is registered under in the runtime. */
export const SETTINGS_ROUTE_ID = "athenaeum-settings"

const CATALOG_PROVIDERS: Partial<Record<LlmConfig["provider"], string>> = {
  openai: "openai",
  anthropic: "anthropic",
  google: "google",
}

function isDeepSeekEndpoint(endpoint: string): boolean {
  return /^https?:\/\/api\.deepseek\.com(?:[:/]|$)/i.test(endpoint.trim())
}

function withKey(route: HarnessRoute, apiKey: string): HarnessRoute {
  return apiKey.trim() ? { ...route, apiKey: apiKey.trim() } : route
}

/** Hand-declared route for an endpoint the harness catalog does not describe. */
function declaredRoute(model: string, api: string, baseURL: string, apiKey: string): HarnessRoute {
  const keyed = apiKey.trim().length > 0
  return withKey({
    provider: SETTINGS_ROUTE_ID,
    model,
    ...(keyed ? { apiKeyEnv: SETTINGS_KEY_ENV } : {}),
    piProvider: {
      displayName: "Athenaeum Settings",
      api,
      baseURL: baseURL.replace(/\/+$/, ""),
      ...(keyed ? { apiKeyEnv: SETTINGS_KEY_ENV } : {}),
      models: [{ id: model }],
    },
  }, apiKey)
}

export function harnessRouteFor(config: LlmConfig): HarnessRouteResult {
  const model = config.model.trim()
  if (!model) return { ok: false, reason: "Choose a model in Settings to use chat." }

  const catalog = CATALOG_PROVIDERS[config.provider]
  if (catalog) {
    return {
      ok: true,
      route: withKey({
        provider: catalog,
        model,
        apiKeyEnv: SETTINGS_KEY_ENV,
        // Listing the model lets an id newer than the harness's catalog through; one the
        // catalog knows still takes its catalog capabilities.
        piProvider: { apiKeyEnv: SETTINGS_KEY_ENV, models: [{ id: model }] },
      }, config.apiKey),
    }
  }

  switch (config.provider) {
    case "custom": {
      const endpoint = config.customEndpoint.trim()
      if (!endpoint) return { ok: false, reason: "Set the endpoint URL in Settings to use chat." }
      // The harness's own DeepSeek adapter knows DeepSeek's thinking format.
      if (isDeepSeekEndpoint(endpoint)) {
        return {
          ok: true,
          route: withKey({ provider: "deepseek-official", model, apiKeyEnv: "DEEPSEEK_API_KEY" }, config.apiKey),
        }
      }
      const api = config.apiMode === "anthropic_messages" ? "anthropic-messages" : "openai-completions"
      return { ok: true, route: declaredRoute(model, api, endpoint, config.apiKey) }
    }
    case "minimax":
      return {
        ok: true,
        route: declaredRoute(model, "anthropic-messages", config.customEndpoint.trim() || "https://api.minimax.io/anthropic", config.apiKey),
      }
    case "ollama": {
      const base = (config.ollamaUrl.trim() || "http://localhost:11434").replace(/\/+$/, "")
      return { ok: true, route: declaredRoute(model, "openai-completions", `${base}/v1`, "") }
    }
    case "azure":
      return { ok: false, reason: "Chat cannot use Azure OpenAI yet. Pick another provider for chat in Settings." }
    default:
      return { ok: false, reason: "Chat cannot use a local CLI provider. Pick an API provider for chat in Settings." }
  }
}

/** Native app-server configuration is isolated from any installed Codex app. */
export function nativeRouteFor(config: LlmConfig): HarnessRouteResult {
  if (config.provider === "openai" && config.apiKey.trim()) {
    return { ok: true, route: { provider: "athenaeum-native", model: config.model.trim() || "native-configuration",
      apiKeyEnv: SETTINGS_KEY_ENV, apiKey: config.apiKey,
      piProvider: { baseURL: "https://api.openai.com/v1" } } }
  }
  // Native provider/auth configuration belongs to ~/.athenaeum-agent/config.toml.
  return { ok: true, route: { provider: "athenaeum-native", model: "native-configuration" } }
}
