/**
 * Consumer plugin: the model-facing `wiki_*` tools over `ctx.wiki`.
 *
 * This package owns schemas, descriptions, prompt guidance and presentation, never a
 * provider. An enabled tool stays visible when no knowledge base is reachable and fails with
 * a structured error at execution time, which is the same contract every other dsh tool
 * keeps: a tool that appears and disappears between turns invalidates the prompt cache and
 * teaches the model nothing it can rely on.
 *
 * @module athenaeum-agent/tool-wiki
 */

import type { Context } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import type {} from '../wiki/index.ts'
import { applyCurationTools } from './curate.ts'
import { applyReadTools } from './read.ts'

export { applyCurationTools } from './curate.ts'
export { knowledgeCall, knowledgeFailure, WikiToolError } from './errors.ts'
export { applyReadTools } from './read.ts'
export * from './project.ts'

/** Cordis plugin name used by loader diagnostics. */
export const name = 'tool-wiki'

/** Services the knowledge tools require. */
export const inject = ['tools', 'wiki', 'systemPrompt']

/** Default cooperative tool-call timeout budget. A graph walk is local work with a hard budget. */
export const DEFAULT_WIKI_TOOL_TIMEOUT_MS = 30_000

/** Which knowledge tools to register and what one call may spend. */
export interface Config {
  /** Register the read tools. Defaults to true. */
  read?: boolean
  /**
   * Register the curation tools. Defaults to false: writing to the knowledge base is a
   * deployment decision, and an agent that records every hunch fills it with noise.
   */
  curation?: boolean
  /** Model name attributed to curation proposals. Required when `curation` is on. */
  curatorModel?: string
  /** Cooperative timeout budget for one knowledge tool call, in milliseconds. A positive integer. */
  timeoutMs?: number
}

export const Config: z<Config> = z.object({
  read: z.boolean().default(true),
  curation: z.boolean().default(false),
  curatorModel: z.string(),
  timeoutMs: z.number().step(1).min(1).default(DEFAULT_WIKI_TOOL_TIMEOUT_MS),
})

/**
 * Register the enabled knowledge tools.
 *
 * @param ctx - plugin context; registrations dispose with it.
 * @param config - which tools to register and their per-call budget.
 * @throws when curation is enabled without a curator model.
 */
export function apply(ctx: Context, config: Config): void {
  const timeoutMs = config.timeoutMs ?? DEFAULT_WIKI_TOOL_TIMEOUT_MS
  if (config.read !== false) applyReadTools(ctx, timeoutMs)
  if (config.curation !== true) return
  const curatorModel = config.curatorModel
  if (curatorModel === undefined || curatorModel.length === 0) {
    // Curation without an attributable author would store model-proposed claims that
    // nothing can later be held to; the library requires the identity for exactly that.
    throw new Error('tool-wiki: curation requires curatorModel, the model name recorded on every proposal')
  }
  applyCurationTools(ctx, curatorModel, timeoutMs)
}
