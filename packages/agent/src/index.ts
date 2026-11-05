/**
 * The Athenaeum agent layer: a DeepSeek Harness bundle that gives a coding agent a project's
 * knowledge base as a capability seam.
 *
 * The four plugins form one complete seam and nothing more. `wiki` declares the capability,
 * `wiki-athenaeum` implements it over Athenaeum Wiki protocol v1, and `tool-wiki` and
 * `wiki-context` consume it — as tools the model calls, and as the brief that opens a task.
 * Mounting the seam without a provider is a legitimate configuration: the agent then works
 * exactly as it does with no knowledge layer installed.
 *
 * `wiki-agent` sits beside them as configuration rather than capability: it declares the Wiki
 * Agent role on the harness's own delegation seam, so the primary agent can delegate one
 * knowledge question to a child on a cheaper model route.
 *
 * This module re-exports the pieces for programmatic use. A running harness mounts the
 * plugins by their subpaths, through `cordis.patch.yml`.
 *
 * @module athenaeum-agent
 */

export * from './protocol/index.ts'
export { WikiRuntime } from './wiki/index.ts'
export type { WikiContextRequest, WikiFailure, WikiProbe, WikiProvider, WikiRuntimeConfig } from './wiki/index.ts'
export {
  probeFromFailure,
  probeFromStatus,
  WIKI_DUPLICATE_PROVIDER,
  WIKI_PROVIDER_AMBIGUOUS,
  WIKI_PROVIDER_CONFIGURED_MISSING,
  WIKI_PROVIDER_CONFIGURED_UNAVAILABLE,
  WIKI_PROVIDER_UNAVAILABLE,
  WikiError,
  wikiFailure,
} from './wiki/index.ts'
export { AthenaeumWikiProvider } from './wiki-athenaeum/provider.ts'
export type { AthenaeumProviderOptions } from './wiki-athenaeum/provider.ts'
export { knowledgeCall, knowledgeFailure, WikiToolError } from './tool-wiki/errors.ts'
export * as wikiAgent from './wiki-agent/index.ts'
