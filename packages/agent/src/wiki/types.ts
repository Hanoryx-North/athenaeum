/**
 * Vocabulary for the knowledge capability seam (`ctx.wiki`).
 *
 * The seam is deliberately thin: a provider answers protocol v1 operations, and everything
 * above it — tools, request context, curation policy — is a consumer. Knowledge types,
 * storage and retrieval belong to the library behind the provider, never here. That
 * direction is what lets the agent runtime stay a legitimate coding agent with no knowledge
 * layer installed at all.
 *
 * @module athenaeum-agent/wiki/types
 */

import { HarnessError } from '@deepseek-ai/dsh-llm'
import {
  BRIDGE_PROTOCOL_MISMATCH,
  BRIDGE_UNSUPPORTED_OPERATION,
  WikiBridgeError,
  WikiRemoteError,
  WikiTransportError,
} from '../protocol/index.ts'
import type { WikiCapabilities, WikiMethod, WikiRequestFor, WikiResponseMap, WikiStatus } from '../protocol/index.ts'

/**
 * One knowledge library the runtime can consult.
 *
 * A provider owns its transport and its own capability negotiation. It reports failures by
 * throwing: a `WikiRemoteError` for a library that answered "no", a `WikiTransportError`
 * for a library that could not be reached.
 */
export interface WikiProvider {
  /** Stable id, unique within the seam. Configuration selects a provider by this. */
  readonly id: string
  /** Cheap local usability check. Must not spawn processes or make network calls. */
  available(): boolean
  /** What this library build serves. Providers may cache the negotiated result. */
  capabilities(signal?: AbortSignal): Promise<WikiCapabilities>
  /** Run one protocol operation. */
  execute<M extends WikiMethod>(request: WikiRequestFor<M>, signal?: AbortSignal): Promise<WikiResponseMap[M]>
}

/** What to retrieve for one task. Budgets are requests; the library caps them to what it advertises. */
export interface WikiContextRequest {
  /** The task in the user's words. The library seeds its walk from this when `seeds` is absent. */
  readonly task: string
  /** Node ids or exact titles to start from. Omit to let the library find seeds. */
  readonly seeds?: readonly string[]
  /** How many hops to walk. */
  readonly depth?: number
  /** Upper bound on nodes in the brief. */
  readonly maxNodes?: number
}

/**
 * Typed failure of the knowledge seam, distinct from a provider's own protocol errors.
 *
 * Every code here is about provider selection, because that is the only thing the seam
 * decides. A library saying "no" arrives as `WikiRemoteError` and passes through unchanged.
 */
export class WikiError extends HarnessError {}

/** A provider id was registered twice. */
export const WIKI_DUPLICATE_PROVIDER = 'WIKI_DUPLICATE_PROVIDER'
/** No usable provider is registered. */
export const WIKI_PROVIDER_UNAVAILABLE = 'WIKI_PROVIDER_UNAVAILABLE'
/** The configured provider id is not registered. */
export const WIKI_PROVIDER_CONFIGURED_MISSING = 'WIKI_PROVIDER_CONFIGURED_MISSING'
/** The configured provider is registered but reports itself unusable. */
export const WIKI_PROVIDER_CONFIGURED_UNAVAILABLE = 'WIKI_PROVIDER_CONFIGURED_UNAVAILABLE'
/** Several usable providers are registered and none is configured. */
export const WIKI_PROVIDER_AMBIGUOUS = 'WIKI_PROVIDER_AMBIGUOUS'

/** Why the knowledge layer cannot answer, in the terms an operator acts on. */
export interface WikiFailure {
  /** Stable machine-routable code: a protocol v1 code, a local bridge code, or a seam code. */
  readonly code: string
  /** What happened, in one sentence, safe to show a person or a model. */
  readonly message: string
}

/**
 * What the knowledge layer can do right now, as a value rather than an exception.
 *
 * The states are deliberately about the deployment, not about one request:
 *
 * - `ready` — a knowledge base answered, with its own status attached.
 * - `absent` — this workspace has no knowledge base. Not a failure, and never a reason to
 *   stop a task: it is the normal state of a project nobody has initialized yet.
 * - `incompatible` — the library speaks a protocol version this adapter does not, or its
 *   build is too old to serve an operation this adapter relies on. The CLI is stale or wrong.
 * - `unreachable` — the command did not run, exceeded its budget, was cancelled, or answered
 *   something that is not a v1 envelope.
 * - `unavailable` — the seam could not choose a provider, or the library answered an error
 *   that is neither "no knowledge base" nor a version problem.
 */
export type WikiProbe =
  | { readonly state: 'ready'; readonly status: WikiStatus }
  | { readonly state: 'absent'; readonly status?: WikiStatus }
  | { readonly state: 'incompatible'; readonly failure: WikiFailure }
  | { readonly state: 'unreachable'; readonly failure: WikiFailure }
  | { readonly state: 'unavailable'; readonly failure: WikiFailure }

/** Describe any thrown value as a routable failure without trusting its shape. */
export function wikiFailure(error: unknown): WikiFailure {
  // WikiError extends HarnessError; bridge failures carry their own `code`. Both are routable,
  // and neither is ever a message to parse.
  if (error instanceof HarnessError) return { code: error.code, message: error.message }
  if (error instanceof WikiBridgeError) return { code: error.code, message: error.message }
  if (error instanceof Error) return { code: 'WIKI_UNKNOWN_FAILURE', message: error.message }
  return { code: 'WIKI_UNKNOWN_FAILURE', message: String(error) }
}

/**
 * Read one status payload as a probe state.
 *
 * A workspace that has never been initialized is not a failure. The library answers a status
 * for it like any other, and that answer is what makes "no knowledge base here" a fact the
 * caller can act on instead of inferring it from an error.
 *
 * @param status - the library's status payload.
 * @returns `ready` when there is a knowledge base to consult, `absent` otherwise.
 */
export function probeFromStatus(status: WikiStatus): WikiProbe {
  return status.initialized && status.hasVault ? { state: 'ready', status } : { state: 'absent', status }
}

/**
 * Classify a failed status call into the state a caller acts on.
 *
 * @param error - whatever the provider threw.
 * @returns the matching probe state; never throws.
 */
export function probeFromFailure(error: unknown): WikiProbe {
  if (error instanceof WikiError) return { state: 'unavailable', failure: wikiFailure(error) }
  if (error instanceof WikiRemoteError) {
    return error.isWorkspaceMissing
      ? { state: 'absent' }
      : { state: 'unavailable', failure: wikiFailure(error) }
  }
  if (error instanceof WikiTransportError) {
    // A version this adapter cannot speak and an operation this build does not serve are both
    // "the installed library is not the one this bundle expects" — the operator's fix differs
    // from a missing command or a timeout, so they do not share a state.
    return error.code === BRIDGE_PROTOCOL_MISMATCH || error.code === BRIDGE_UNSUPPORTED_OPERATION
      ? { state: 'incompatible', failure: wikiFailure(error) }
      : { state: 'unreachable', failure: wikiFailure(error) }
  }
  return { state: 'unreachable', failure: wikiFailure(error) }
}
