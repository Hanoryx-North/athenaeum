/**
 * Service Provider plugin: registers an Athenaeum knowledge base on `ctx.wiki`.
 *
 * Mounting this plugin is the whole installation step. Nothing else in the runtime learns
 * that Athenaeum exists; consumers see only the seam.
 *
 * @module athenaeum-agent/wiki-athenaeum
 */

import type { Context } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import type {} from '../wiki/index.ts'
import { AthenaeumWikiProvider } from './provider.ts'

export { AthenaeumWikiProvider } from './provider.ts'
export type { AthenaeumProviderOptions } from './provider.ts'

/** Cordis plugin name used by loader diagnostics. */
export const name = 'wiki-athenaeum'

/** The knowledge seam this provider registers on. */
export const inject = ['wiki']

/** Default seam registry id for a provider backed by the `athenaeum` command. */
export const DEFAULT_PROVIDER_ID = 'athenaeum'

/** Default wall-clock budget for one bridge call. A knowledge walk is bounded work, not a model call. */
export const DEFAULT_BRIDGE_TIMEOUT_MS = 30_000

/** Default cap on one bridge response. A full-budget brief is orders of magnitude smaller. */
export const DEFAULT_MAX_RESPONSE_BYTES = 4 * 1024 * 1024

/** Where the knowledge base is and how much one call may spend. */
export interface Config {
  /** Seam registry id. Set this when mounting more than one knowledge base. */
  id?: string
  /** The Athenaeum executable. A bare name resolves through `PATH`. */
  command?: string
  /** The project directory whose knowledge base is addressed. Defaults to the runtime's working directory. */
  workspace?: string
  /** Wall-clock budget for one bridge call, in milliseconds. A positive integer. */
  timeoutMs?: number
  /** Cap on bytes read from one bridge response. A positive integer. */
  maxResponseBytes?: number
}

export const Config: z<Config> = z.object({
  id: z.string().default(DEFAULT_PROVIDER_ID),
  command: z.string().default('athenaeum'),
  workspace: z.string(),
  timeoutMs: z.number().step(1).min(1).default(DEFAULT_BRIDGE_TIMEOUT_MS),
  maxResponseBytes: z.number().step(1).min(1).default(DEFAULT_MAX_RESPONSE_BYTES),
})

/**
 * Register the provider for the lifetime of `ctx`.
 *
 * The workspace defaults to the runtime's working directory, which is the project the agent
 * was pointed at. An explicit `workspace` addresses a knowledge base somewhere else.
 *
 * @param ctx - plugin context; the registration is disposed with it.
 * @param config - knowledge base location and per-call budgets.
 */
export function apply(ctx: Context, config: Config): void {
  const id = config.id ?? DEFAULT_PROVIDER_ID
  const command = config.command ?? 'athenaeum'
  const workspace = config.workspace ?? process.cwd()
  const timeoutMs = config.timeoutMs ?? DEFAULT_BRIDGE_TIMEOUT_MS
  const maxResponseBytes = config.maxResponseBytes ?? DEFAULT_MAX_RESPONSE_BYTES
  ctx.wiki.registerProvider(new AthenaeumWikiProvider({ id, command, workspace, timeoutMs, maxResponseBytes }))
}
