/**
 * Service Definition for the knowledge capability seam (`ctx.wiki`): a provider registry and
 * execution-time provider selection over Athenaeum Wiki protocol v1.
 *
 * Selection is resolved per call and never depends on registration order, so mounting a
 * second library cannot silently change which one answers.
 *
 * @module athenaeum-agent/wiki
 */

import { Context, Service } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import { WikiRemoteError } from '../protocol/index.ts'
import type {
  WikiCapabilities,
  WikiContext,
  WikiMethod,
  WikiRequestFor,
  WikiResponseMap,
} from '../protocol/index.ts'
import {
  WIKI_PROVIDER_AMBIGUOUS,
  WIKI_PROVIDER_CONFIGURED_MISSING,
  WIKI_PROVIDER_CONFIGURED_UNAVAILABLE,
  WIKI_PROVIDER_UNAVAILABLE,
  WIKI_DUPLICATE_PROVIDER,
  WikiError,
  probeFromFailure,
  probeFromStatus,
} from './types.ts'
import type { WikiContextRequest, WikiProbe, WikiProvider } from './types.ts'

export {
  WIKI_DUPLICATE_PROVIDER,
  WIKI_PROVIDER_AMBIGUOUS,
  WIKI_PROVIDER_CONFIGURED_MISSING,
  WIKI_PROVIDER_CONFIGURED_UNAVAILABLE,
  WIKI_PROVIDER_UNAVAILABLE,
  WikiError,
  probeFromFailure,
  probeFromStatus,
  wikiFailure,
} from './types.ts'
export type { WikiContextRequest, WikiFailure, WikiProbe, WikiProvider } from './types.ts'

declare module '@deepseek-ai/cordis' {
  interface Context {
    wiki: WikiRuntime
  }
}

/** Config for the knowledge seam. */
export interface WikiRuntimeConfig {
  /** Explicit provider id. Omitted = auto-select when exactly one registered provider is usable. */
  readonly provider?: string
}

/**
 * The knowledge service, registered as `ctx.wiki`.
 *
 * Selection, resolved at execution time:
 * - a configured id that is registered and usable wins;
 * - a configured id that is missing or unusable fails loudly rather than falling back,
 *   because silently consulting a different knowledge base is worse than not consulting one;
 * - with no id configured, exactly one usable provider wins;
 * - anything else fails with the matching {@link WikiError} code.
 */
export class WikiRuntime extends Service {
  static Config: z<WikiRuntimeConfig> = z.object({
    provider: z.string(),
  })

  private readonly providers = new Map<string, WikiProvider>()
  private readonly configuredId: string | undefined

  constructor(ctx: Context, config: WikiRuntimeConfig = {}) {
    super(ctx, 'wiki')
    this.configuredId = config.provider
  }

  /**
   * Register one knowledge provider.
   *
   * @param provider - the provider; its `id` is the registry key.
   * @returns the disposer that unregisters it; also disposed with the calling fiber.
   * @throws {WikiError} `WIKI_DUPLICATE_PROVIDER` when the id is already registered.
   */
  registerProvider(provider: WikiProvider): () => void {
    if (this.providers.has(provider.id)) {
      throw new WikiError(`a wiki provider with id "${provider.id}" is already registered`, WIKI_DUPLICATE_PROVIDER)
    }
    const providers = this.providers
    const dispose = this.ctx.effect(function* () {
      providers.set(provider.id, provider)
      yield () => providers.delete(provider.id)
    }, 'wiki.registerProvider()')
    // ctx.effect's disposer resolves a promise; the seam's disposer API is synchronous.
    return () => void dispose()
  }

  /** Ids of every registered provider, usable or not. */
  providerIds(): readonly string[] {
    return [...this.providers.keys()]
  }

  /** True when a provider would answer a call right now — the cheap check a consumer uses before offering knowledge tools. */
  hasUsableProvider(): boolean {
    try {
      this.resolve()
      return true
    } catch {
      // Selection failures are the answer to this question, not an error to report.
      return false
    }
  }

  /**
   * Run one protocol operation against the selected provider.
   *
   * @param request - the operation and its arguments.
   * @param signal - optional cancellation forwarded to the provider.
   * @returns the response payload for that method.
   * @throws {WikiError} when no provider can be selected.
   */
  async execute<M extends WikiMethod>(request: WikiRequestFor<M>, signal?: AbortSignal): Promise<WikiResponseMap[M]> {
    return this.resolve().execute(request, signal)
  }

  /**
   * What the selected library build serves.
   *
   * @param signal - optional cancellation forwarded to the provider.
   * @returns the advertised capabilities.
   * @throws {WikiError} when no provider can be selected.
   */
  async capabilities(signal?: AbortSignal): Promise<WikiCapabilities> {
    return this.resolve().capabilities(signal)
  }

  /**
   * Ask what the knowledge layer can do right now.
   *
   * This is the one seam call a consumer makes before offering knowledge at all, and it is
   * the reason it never throws: "no knowledge base", "the command is not installed", "the
   * CLI is older than this bundle" and "the library answered" are all ordinary answers about
   * a deployment, and a caller that only wants to decide whether to mention knowledge should
   * not have to catch exceptions to hear them.
   *
   * A missing knowledge base is `absent`, not an error. Every other state names what to fix.
   *
   * @param signal - optional cancellation forwarded to the provider.
   * @returns the probe state and, when the library answered, its status payload.
   */
  async probe(signal?: AbortSignal): Promise<WikiProbe> {
    let response: WikiResponseMap['status']
    try {
      response = await this.execute({ method: 'status' }, signal)
    } catch (error: unknown) {
      return probeFromFailure(error)
    }
    return probeFromStatus(response.status)
  }

  /**
   * Retrieve knowledge for one task.
   *
   * Returns `undefined` in the two cases where a consumer must add nothing to the request:
   * a workspace with no knowledge base at all, and a knowledge base that had nothing to say.
   * An empty brief injected as context would tell the model the project has no conventions,
   * which is a stronger claim than "nothing is recorded yet".
   *
   * @param request - the task and its retrieval budgets.
   * @param signal - optional cancellation forwarded to the provider.
   * @returns the brief and its trust-wrapped prompt, or `undefined` when there is nothing to add.
   * @throws {WikiError} when no provider can be selected; provider failures propagate unchanged.
   */
  async context(request: WikiContextRequest, signal?: AbortSignal): Promise<WikiContext | undefined> {
    let response: WikiResponseMap['context']
    try {
      response = await this.execute({
        method: 'context',
        task: request.task,
        ...request.seeds === undefined ? {} : { seeds: request.seeds },
        ...request.depth === undefined ? {} : { depth: request.depth },
        ...request.maxNodes === undefined ? {} : { maxNodes: request.maxNodes },
      }, signal)
    } catch (error: unknown) {
      if (error instanceof WikiRemoteError && error.isWorkspaceMissing) return undefined
      throw error
    }
    return response.context.brief.nodes.length === 0 ? undefined : response.context
  }

  /**
   * Report which retrieved nodes actually helped.
   *
   * Utility is what later distinguishes knowledge that keeps being retrieved and never used
   * from knowledge that earns its place. A workspace with no knowledge base is not an error
   * here — there is simply nothing to journal.
   *
   * @param nodes - node ids or exact titles that contributed to the finished work.
   * @param session - this runtime's opaque session id, journaled as the reporter.
   * @param signal - optional cancellation forwarded to the provider.
   * @throws {WikiError} when no provider can be selected; provider failures propagate unchanged.
   */
  async recordUse(nodes: readonly string[], session?: string, signal?: AbortSignal): Promise<void> {
    if (nodes.length === 0) return
    try {
      await this.execute({
        method: 'record_use',
        nodes,
        ...session === undefined ? {} : { session },
      }, signal)
    } catch (error: unknown) {
      if (error instanceof WikiRemoteError && error.isWorkspaceMissing) return
      throw error
    }
  }

  /** Select the provider for this call, or throw the matching {@link WikiError}. */
  private resolve(): WikiProvider {
    if (this.configuredId !== undefined) {
      const configured = this.providers.get(this.configuredId)
      if (configured === undefined) {
        throw new WikiError(
          `configured wiki provider "${this.configuredId}" is not registered`,
          WIKI_PROVIDER_CONFIGURED_MISSING,
        )
      }
      if (!configured.available()) {
        throw new WikiError(
          `configured wiki provider "${this.configuredId}" is registered but unavailable`,
          WIKI_PROVIDER_CONFIGURED_UNAVAILABLE,
        )
      }
      return configured
    }
    const usable = [...this.providers.values()].filter(provider => provider.available())
    const [single] = usable
    if (single === undefined) throw new WikiError('no usable wiki provider is registered', WIKI_PROVIDER_UNAVAILABLE)
    if (usable.length > 1) {
      const ids = usable.map(provider => provider.id).join(', ')
      throw new WikiError(
        `multiple usable wiki providers are registered (${ids}); configure one explicitly`,
        WIKI_PROVIDER_AMBIGUOUS,
      )
    }
    return single
  }
}

export default WikiRuntime
