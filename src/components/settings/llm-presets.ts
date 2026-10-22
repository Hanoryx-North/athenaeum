import type { AzureModelFamily, CustomLlmPreset } from "@/stores/wiki-store"

/**
 * Curated LLM provider presets.
 *
 * Selecting a preset pre-fills the underlying LlmConfig fields so users
 * don't have to remember endpoint URLs / API mode per vendor. The
 * dispatch code in `src/lib/llm-providers.ts` still branches on the
 * lower-level `provider` field — presets just populate the config.
 */
export type CustomApiMode = "chat_completions" | "anthropic_messages"

export type Provider =
  | "openai"
  | "anthropic"
  | "google"
  | "azure"
  | "ollama"
  | "custom"
  | "minimax"
  | "claude-code"
  | "codex-cli"

export interface LlmPreset {
  /** Stable id used as the dropdown value. */
  id: string
  /** Display label in the dropdown. */
  label: string
  /** Short subtitle shown under the label. */
  hint?: string
  /** Underlying provider dispatch key (see llm-providers.ts). */
  provider: Provider
  /** Suggested base URL. `customEndpoint` for custom, `ollamaUrl` for ollama, ignored for built-ins. */
  baseUrl?: string
  /**
   * For vendors that serve the same model catalog over both an OpenAI-
   * compatible and an Anthropic-compatible endpoint at different URLs
   * (e.g. Alibaba Bailian Coding Plan), list the URL per wire mode.
   * The settings UI auto-swaps `baseUrl` when the user flips the API
   * mode toggle — so one preset covers both protocols instead of two.
   */
  baseUrlByMode?: Partial<Record<CustomApiMode, string>>
  /** Suggested default model; user can override. */
  defaultModel?: string
  /** Azure OpenAI api-version query parameter. Azure deployments vary by resource. */
  azureApiVersion?: string
  /** Azure deployment names are arbitrary, so users can declare GPT-5/o-series behavior explicitly. */
  azureModelFamily?: AzureModelFamily
  /**
   * Curated list of model ids the UI shows as clickable chips above the
   * Model input. The user can still type a custom value — the input stays
   * free-form. An empty/missing list means "no suggestions, type freely"
   * (e.g. Ollama Local where the model set is whatever the user pulled).
   */
  suggestedModels?: string[]
  /** Custom providers only: which wire protocol to speak. */
  apiMode?: CustomApiMode
  /** Suggested context window; user can override. */
  suggestedContextSize?: number
}

export const LLM_PRESETS: LlmPreset[] = [
  {
    id: "anthropic",
    label: "Anthropic (Claude)",
    hint: "Official Claude API",
    provider: "anthropic",
    defaultModel: "claude-sonnet-5-5",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    // Sonnet 5.5 / Haiku 5.5 are newer than that catalog.
    suggestedModels: [
      "claude-opus-5-5",
      "claude-sonnet-5-5",
      "claude-haiku-5-5",
      "claude-fable-5-1",
      "claude-opus-5",
      "claude-sonnet-5",
      "claude-opus-4-8",
      "claude-sonnet-4-6",
      "claude-haiku-4-5",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "claude-code-cli",
    label: "Claude Code CLI (local)",
    hint: "Uses the local `claude` binary — no API key needed",
    provider: "claude-code",
    defaultModel: "claude-sonnet-5-5",
    // Same ids as the Anthropic preset: the CLI forwards to the same backend.
    suggestedModels: [
      "claude-opus-5-5",
      "claude-sonnet-5-5",
      "claude-haiku-5-5",
      "claude-fable-5-1",
      "claude-opus-5",
      "claude-sonnet-5",
      "claude-opus-4-8",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "codex-cli",
    label: "Codex CLI (local)",
    hint: "Uses the local `codex` binary — no API key needed",
    provider: "codex-cli",
    defaultModel: "gpt-5.5",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first. (openai-codex)
    suggestedModels: [
      "gpt-6-sol",
      "gpt-6-luna",
      "gpt-6-astra",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.3-codex-spark",
    ],
    suggestedContextSize: 272000,
  },
  {
    id: "openai",
    label: "OpenAI (GPT)",
    hint: "Official OpenAI API",
    provider: "openai",
    defaultModel: "gpt-5.5",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "gpt-6-sol",
      "gpt-6-luna",
      "gpt-6-astra",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.5-pro",
      "gpt-5.4-mini",
      "gpt-5.4-nano",
      "gpt-4.1",
      "gpt-4o-mini",
    ],
    suggestedContextSize: 272000,
  },
  {
    id: "google",
    label: "Google (Gemini)",
    hint: "Generative Language API",
    provider: "google",
    defaultModel: "gemini-3.8-flash",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    // The 2.x line is closed to new API keys ("no longer available to new users").
    suggestedModels: [
      "gemini-3.8-flash",
      "gemini-3.7-flash",
      "gemini-3.5-flash",
      "gemini-3.5-flash-lite",
      "gemini-3.1-pro-preview",
      "gemini-3-flash-preview",
      "gemini-flash-latest",
      "gemini-flash-lite-latest",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "azure",
    label: "Azure OpenAI",
    hint: "Azure OpenAI resource endpoint; Model field is the deployment name",
    provider: "azure",
    baseUrl: "https://your-resource.openai.azure.com",
    defaultModel: "your-deployment-name",
    azureApiVersion: "2024-10-21",
    suggestedContextSize: 128000,
  },
  {
    id: "deepseek",
    label: "DeepSeek",
    hint: "api.deepseek.com",
    provider: "custom",
    baseUrl: "https://api.deepseek.com/v1",
    defaultModel: "deepseek-v4-flash",
    apiMode: "chat_completions",
    // deepseek-flash is V4.1 Flash. deepseek-chat / deepseek-reasoner were retired on
    // 2026-07-24; type either id in if a gateway still serves it.
    suggestedModels: [
      "deepseek-v4-flash",
      "deepseek-flash",
      "deepseek-v4-pro",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "atlascloud",
    label: "Atlas Cloud",
    hint: "api.atlascloud.ai",
    provider: "custom",
    baseUrl: "https://api.atlascloud.ai/v1",
    defaultModel: "deepseek-ai/deepseek-v4-pro",
    apiMode: "chat_completions",
    // Atlas Cloud is a full-modal inference platform exposing many model
    // families (DeepSeek, Qwen, GLM, Kimi, MiniMax, Claude, GPT, Gemini…)
    // behind a single OpenAI-compatible /v1/chat/completions endpoint, so
    // it reuses the generic chat-completions wire like the other hosted
    // gateways above. `deepseek-v4-pro` is a reasoning model — leave the
    // context window generous. Full catalog is large and rotates; this is
    // a practical subset and users can type any other id into the input.
    suggestedModels: [
      "deepseek-ai/deepseek-v4-pro",
      "deepseek-ai/deepseek-v4-flash",
      "deepseek-ai/deepseek-v3.2",
      "Qwen/Qwen3-Next-80B-A3B-Instruct",
      "moonshotai/kimi-k2.6",
      "zai-org/glm-5",
      "minimaxai/minimax-m2.7",
      "anthropic/claude-sonnet-4.6",
      "openai/gpt-5.5",
      "google/gemini-3.5-flash",
    ],
    suggestedContextSize: 128000,
  },
  {
    id: "groq",
    label: "Groq",
    hint: "api.groq.com",
    provider: "custom",
    baseUrl: "https://api.groq.com/openai/v1",
    defaultModel: "llama-3.3-70b-versatile",
    apiMode: "chat_completions",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "llama-3.3-70b-versatile",
      "llama-3.1-8b-instant",
      "openai/gpt-oss-120b",
      "openai/gpt-oss-20b",
      "qwen/qwen3.8-27b",
      "qwen/qwen3.6-27b",
    ],
    suggestedContextSize: 128000,
  },
  {
    id: "xai",
    label: "xAI (Grok)",
    hint: "api.x.ai",
    provider: "custom",
    baseUrl: "https://api.x.ai/v1",
    defaultModel: "grok-4.7",
    apiMode: "chat_completions",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "grok-4.7",
      "grok-4.6",
      "grok-4.5",
      "grok-4.3",
    ],
    suggestedContextSize: 500000,
  },
  {
    id: "nvidia-nim",
    label: "NVIDIA NIM",
    hint: "integrate.api.nvidia.com",
    provider: "custom",
    baseUrl: "https://integrate.api.nvidia.com/v1",
    apiMode: "chat_completions",
    // NVIDIA's build.nvidia.com hosts both their own Nemotron family
    // and a rotating catalog of popular third-party open-weight models
    // behind the same OpenAI-compatible endpoint. API key is issued
    // per-user from build.nvidia.com. Full catalog is huge and
    // changes often — this is a practical subset; users can type any
    // other id into the custom input.
    defaultModel: "nvidia/nemotron-3-super-120b-a12b",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    // The hosted catalog rotates; any other id can be typed in.
    suggestedModels: [
      "nvidia/nemotron-3-ultra-550b-a55b",
      "nvidia/nemotron-3-super-120b-a12b",
      "nvidia/nemotron-3.5-lightning-30b-a3b",
      "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning",
      "nvidia/llama-3.1-nemotron-ultra-253b-v1",
      "moonshotai/kimi-k3",
      "moonshotai/kimi-k2.6",
      "z-ai/glm-5.3",
      "z-ai/glm-5.3-flash",
      "openai/gpt-oss-20b",
      "meta/llama-3.2-90b-vision-instruct",
    ],
    suggestedContextSize: 128000,
  },
  {
    id: "kimi",
    label: "Kimi (Moonshot)",
    hint: "api.moonshot.ai",
    provider: "custom",
    baseUrl: "https://api.moonshot.ai/v1",
    defaultModel: "kimi-k3",
    apiMode: "chat_completions",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "kimi-k3",
      "kimi-k2.7-code",
      "kimi-k2.7-code-highspeed",
      "kimi-k2.6",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "kimi-cn",
    label: "Kimi (Moonshot, 中国)",
    hint: "api.moonshot.cn",
    provider: "custom",
    baseUrl: "https://api.moonshot.cn/v1",
    defaultModel: "kimi-k3",
    apiMode: "chat_completions",
    suggestedModels: [
      "kimi-k3",
      "kimi-k2.7-code",
      "kimi-k2.7-code-highspeed",
      "kimi-k2.6",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "kimi-coding-plan",
    label: "Kimi (Coding Plan)",
    hint: "api.kimi.com",
    provider: "custom",
    baseUrl: "https://api.kimi.com/coding/",
    defaultModel: "kimi-for-coding",
    apiMode: "chat_completions",
    // Kimi Coding Plan is a separate subscription from the Moonshot open platform,
    // with both OpenAI- and Anthropic-compatible wires on this base URL (the Anthropic
    // wire needs Bearer auth; see requiresBearerAuth in llm-providers.ts). Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "kimi-for-coding",
      "kimi-for-coding-highspeed",
      "k3",
      "k3-256k",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "zhipu",
    label: "智谱 GLM (Zhipu)",
    hint: "open.bigmodel.cn",
    provider: "custom",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    defaultModel: "glm-5.3",
    apiMode: "chat_completions",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first. (zai)
    // Older glm-4.x / glm-5 / glm-5.1 ids still work if typed in.
    suggestedModels: [
      "glm-5.3",
      "glm-5.3-flash",
      "glm-5.3-highspeed",
      "glm-5.2",
      "glm-5.2-highspeed",
      "glm-5-turbo",
      "glm-4.7",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "minimax-global",
    label: "MiniMax (Global)",
    hint: "api.minimax.io/anthropic",
    provider: "custom",
    baseUrl: "https://api.minimax.io/anthropic",
    defaultModel: "MiniMax-M3",
    apiMode: "anthropic_messages",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "MiniMax-M3",
      "MiniMax-M2.7",
      "MiniMax-M2.7-highspeed",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "minimax-cn",
    label: "MiniMax (中国)",
    hint: "api.minimaxi.com/anthropic",
    provider: "custom",
    baseUrl: "https://api.minimaxi.com/anthropic",
    defaultModel: "MiniMax-M3",
    apiMode: "anthropic_messages",
    suggestedModels: [
      "MiniMax-M3",
      "MiniMax-M2.7",
      "MiniMax-M2.7-highspeed",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "bailian-coding",
    label: "阿里百炼 Coding Plan",
    hint: "coding.dashscope.aliyuncs.com",
    provider: "custom",
    // Default wire is OpenAI-compat. Flipping the "API 模式" toggle to
    // Anthropic-compat auto-swaps the base URL via baseUrlByMode below,
    // so users don't have to know the two URLs exist.
    baseUrl: "https://coding.dashscope.aliyuncs.com/v1",
    apiMode: "chat_completions",
    baseUrlByMode: {
      chat_completions: "https://coding.dashscope.aliyuncs.com/v1",
      anthropic_messages: "https://coding.dashscope.aliyuncs.com/apps/anthropic",
    },
    // Bailian's subscription-only "Coding Plan" exposes the same model
    // catalog on both wires. Key must come from the Bailian console's
    // Coding Plan tab — a regular DashScope key will 401. The
    // Anthropic-compat path uses Bearer auth (see requiresBearerAuth
    // in llm-providers.ts), matching the MiniMax gateway convention.
    defaultModel: "qwen3.6-plus",
    suggestedModels: [
      "qwen3.6-plus",
      "kimi-k2.5",
      "glm-5",
      "MiniMax-M2.5",
      "qwen3.5-plus",
      "qwen3-max-2026-01-23",
      "qwen3-coder-plus",
      "qwen3-coder-next",
      "glm-4.7",
    ],
    suggestedContextSize: 131072,
  },
  {
    id: "xiaomi-mimo",
    label: "小米 MiMo (Xiaomi)",
    hint: "api.xiaomimimo.com",
    provider: "custom",
    baseUrl: "https://api.xiaomimimo.com/v1",
    apiMode: "chat_completions",
    baseUrlByMode: {
      chat_completions: "https://token-plan-cn.xiaomimimo.com/v1",
      anthropic_messages: "https://token-plan-cn.xiaomimimo.com/anthropic",
    },
    // Official OpenAI-compatible endpoint at api.xiaomimimo.com/v1.
    // Token Plan users can switch API mode to the CN OpenAI/Anthropic
    // gateways above. MiMo V2.5 Pro / Omni advertise 1M context;
    // Flash remains the low-cost 256K option. Older v2 ids stay
    // selectable for existing users and gateway deployments.
    defaultModel: "mimo-v2.6-pro",
    // Model ids from the harness's pi-ai catalog (0.87.1, Oct 2026), newest first.
    suggestedModels: [
      "mimo-v2.6-pro",
      "mimo-v2.6-flash",
      "mimo-v2.6-pro-ultraspeed",
      "mimo-v2.5-pro",
      "mimo-v2.5",
    ],
    suggestedContextSize: 1000000,
  },
  {
    id: "volcengine-ark",
    label: "火山引擎 Ark (Volcengine)",
    hint: "ark.cn-beijing.volces.com/api/coding/v3",
    provider: "custom",
    // Volcengine Ark's "coding" product line mandates this exact base URL
    // per their official docs. Their other OpenAI-compat base (api/v3) is
    // not a valid substitute. This endpoint rejects browser-origin fetch
    // via CORS (allow-headers omits `authorization`), so LLM calls route
    // through Tauri's HTTP plugin — see src/lib/llm-client.ts.
    baseUrl: "https://ark.cn-beijing.volces.com/api/coding/v3",
    apiMode: "chat_completions",
    // Exact model catalog Volcengine's Coding product line accepts on
    // this endpoint. The older `doubao-*-1-5-*` / `doubao-seed-1-6-*` /
    // `deepseek-v3-250324` ids that work on the general Ark endpoints
    // are NOT available here and will 400.
    defaultModel: "Doubao-Seed-2.0-Code",
    suggestedModels: [
      "Doubao-Seed-2.0-Code",
      "Doubao-Seed-2.0-pro",
      "Doubao-Seed-2.0-lite",
      "Doubao-Seed-Code",
      "MiniMax-M2.5",
      "Kimi-K2.5",
      "GLM-4.7",
      "DeepSeek-V3",
    ],
    suggestedContextSize: 128000,
  },
  {
    id: "ollama-local",
    label: "Ollama (Local)",
    hint: "Self-hosted llama.cpp / Ollama",
    provider: "ollama",
    baseUrl: "http://localhost:11434",
    // Intentionally no suggestedModels: local set depends on what the
    // user has actually pulled / loaded. Kept as free-text input.
    suggestedContextSize: 32768,
  },
  {
    id: "ollama-cloud",
    label: "Ollama Cloud",
    hint: "ollama.com",
    provider: "custom",
    baseUrl: "https://ollama.com/v1",
    apiMode: "chat_completions",
    // Ollama Cloud catalog rotates frequently — keep short common picks.
    suggestedModels: [
      "gpt-oss:120b",
      "gpt-oss:20b",
      "qwen3-coder:480b",
      "kimi-k2:1t",
      "deepseek-v3.1:671b",
    ],
    suggestedContextSize: 128000,
  },
  {
    id: "custom",
    label: "Custom",
    hint: "Any OpenAI- or Anthropic-compatible endpoint",
    provider: "custom",
    // Wire protocol is chosen via the "API 模式" toggle in the expanded
    // panel — no need for separate presets per mode. User supplies the
    // base URL manually (no baseUrlByMode: nothing we can auto-fill).
    apiMode: "chat_completions",
    // No suggestedModels: user knows what their gateway exposes.
  },
]

export function availableLlmPresets(customPresets: CustomLlmPreset[] = []): LlmPreset[] {
  return [
    ...LLM_PRESETS,
    ...customPresets.map((preset) => ({
      id: preset.id,
      label: preset.label,
      hint: "Custom OpenAI- or Anthropic-compatible endpoint",
      provider: "custom" as const,
      apiMode: "chat_completions" as const,
    })),
  ]
}

export function findLlmPreset(id: string, customPresets: CustomLlmPreset[] = []): LlmPreset | undefined {
  return availableLlmPresets(customPresets).find((preset) => preset.id === id)
}

/**
 * Best-effort reverse lookup: given the current LlmConfig fields, which
 * preset does it most likely correspond to? Used so the dropdown can
 * show the user what they're effectively on.
 */
export function matchPreset(params: {
  provider: Provider
  customEndpoint: string
  ollamaUrl: string
  apiMode?: CustomApiMode
}): LlmPreset | null {
  const norm = (u: string) => u.replace(/\/+$/, "").toLowerCase()
  const { provider, customEndpoint, ollamaUrl, apiMode } = params

  for (const preset of LLM_PRESETS) {
    if (preset.provider !== provider) continue
    if (provider === "custom") {
      if (!preset.baseUrl) continue // skip the generic Custom catch-alls
      if (norm(preset.baseUrl) !== norm(customEndpoint)) continue
      if ((preset.apiMode ?? "chat_completions") !== (apiMode ?? "chat_completions"))
        continue
      return preset
    }
    if (provider === "ollama") {
      if (preset.baseUrl && norm(preset.baseUrl) !== norm(ollamaUrl)) continue
      return preset
    }
    return preset
  }
  return null
}
