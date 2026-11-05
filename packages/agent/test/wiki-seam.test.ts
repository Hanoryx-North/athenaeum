import { describe, expect, it } from 'vitest'
import { Context } from '@deepseek-ai/cordis'
import type { WikiCapabilities, WikiMethod, WikiRequestFor, WikiResponseMap } from '../src/protocol/index.ts'
import { WikiRemoteError, WikiTransportError } from '../src/protocol/index.ts'
import WikiRuntime, {
  WIKI_DUPLICATE_PROVIDER,
  WIKI_PROVIDER_AMBIGUOUS,
  WIKI_PROVIDER_CONFIGURED_MISSING,
  WIKI_PROVIDER_CONFIGURED_UNAVAILABLE,
  WIKI_PROVIDER_UNAVAILABLE,
} from '../src/wiki/index.ts'
import type { WikiProvider } from '../src/wiki/index.ts'

const CAPABILITIES: WikiCapabilities = {
  protocolVersion: 1,
  operations: ['capabilities', 'context', 'search', 'record_use'],
  limits: { maxSearchResults: 100, maxContextSeeds: 10, maxContextDepth: 4, maxContextNodes: 100 },
  nodeTypes: ['decision'],
  edgeTypes: ['has_gotcha'],
  authorities: ['project_rule'],
  scopes: ['project'],
  evidenceKinds: ['source_file'],
}

/** A provider whose answers are scripted per method. */
function provider(
  id: string,
  available: boolean,
  answers: Partial<{ [M in WikiMethod]: (request: WikiRequestFor<M>) => Promise<WikiResponseMap[M]> }> = {},
): WikiProvider & { calls: WikiMethod[] } {
  const calls: WikiMethod[] = []
  return {
    id,
    calls,
    available: () => available,
    capabilities: () => Promise.resolve(CAPABILITIES),
    execute<M extends WikiMethod>(request: WikiRequestFor<M>): Promise<WikiResponseMap[M]> {
      calls.push(request.method)
      const answer = answers[request.method] as ((r: WikiRequestFor<M>) => Promise<WikiResponseMap[M]>) | undefined
      if (answer === undefined) return Promise.reject(new Error(`unscripted ${request.method}`))
      return answer(request)
    },
  }
}

/** A `context` answer carrying the given brief nodes. */
function contextAnswer(nodes: { id: string; title: string }[]): () => Promise<WikiResponseMap['context']> {
  return () => Promise.resolve({
    method: 'context',
    context: {
      brief: {
        task: 'fix the OAuth callback',
        nodes: nodes.map(node => ({ ...node, type: 'decision', summary: '', authority: 'project_decision', status: 'valid', depth: 0 })),
        relationships: [],
        anchors: [],
        warnings: [],
        truncated: false,
      },
      prompt: '<retrieved_knowledge>…</retrieved_knowledge>',
    },
  })
}

/** Mount the seam on a fresh root context. */
async function mountWiki(config: ConstructorParameters<typeof WikiRuntime>[1] = {}): Promise<WikiRuntime> {
  const ctx = new Context()
  await ctx.plugin(WikiRuntime, config)
  return ctx.wiki
}

describe('provider registration', () => {
  it('registers a provider and unregisters it through the returned disposer', async () => {
    const wiki = await mountWiki()

    const dispose = wiki.registerProvider(provider('athenaeum', true, { capabilities: () => Promise.resolve({ method: 'capabilities', capabilities: CAPABILITIES }) }))
    await expect(wiki.capabilities()).resolves.toMatchObject({ protocolVersion: 1 })
    expect(wiki.providerIds()).toEqual(['athenaeum'])

    dispose()

    expect(wiki.providerIds()).toEqual([])
    await expect(wiki.capabilities()).rejects.toThrow(expect.objectContaining({ code: WIKI_PROVIDER_UNAVAILABLE }))
  })

  it('refuses a duplicate provider id', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true))

    expect(() => wiki.registerProvider(provider('athenaeum', true)))
      .toThrow(expect.objectContaining({ code: WIKI_DUPLICATE_PROVIDER }))
  })
})

describe('provider selection', () => {
  it('auto-selects the only usable provider', async () => {
    const wiki = await mountWiki()
    const only = provider('athenaeum', true, { context: contextAnswer([{ id: 'n1', title: 'Validate state first' }]) })
    wiki.registerProvider(only)
    wiki.registerProvider(provider('offline', false))

    await wiki.context({ task: 'fix the OAuth callback' })

    expect(only.calls).toEqual(['context'])
  })

  it('refuses to guess between several usable providers', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true))
    wiki.registerProvider(provider('second', true))

    await expect(wiki.capabilities()).rejects.toThrow(expect.objectContaining({ code: WIKI_PROVIDER_AMBIGUOUS }))
    expect(wiki.hasUsableProvider()).toBe(false)
  })

  it('fails loudly when the configured provider is missing rather than consulting another', async () => {
    const wiki = await mountWiki({ provider: 'athenaeum' })
    wiki.registerProvider(provider('some-other-library', true))

    await expect(wiki.capabilities()).rejects.toThrow(expect.objectContaining({ code: WIKI_PROVIDER_CONFIGURED_MISSING }))
  })

  it('fails loudly when the configured provider is registered but unusable', async () => {
    const wiki = await mountWiki({ provider: 'athenaeum' })
    wiki.registerProvider(provider('athenaeum', false))

    await expect(wiki.capabilities()).rejects.toThrow(expect.objectContaining({ code: WIKI_PROVIDER_CONFIGURED_UNAVAILABLE }))
  })
})

describe('context retrieval', () => {
  it('returns the library context when the brief holds knowledge', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true, { context: contextAnswer([{ id: 'n1', title: 'Validate state first' }]) }))

    const context = await wiki.context({ task: 'fix the OAuth callback', seeds: ['OAuth'], depth: 2, maxNodes: 25 })

    expect(context?.brief.nodes).toHaveLength(1)
    expect(context?.prompt).toContain('<retrieved_knowledge>')
  })

  it('adds nothing when the knowledge base had nothing to say', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true, { context: contextAnswer([]) }))

    await expect(wiki.context({ task: 'fix the OAuth callback' })).resolves.toBeUndefined()
  })

  it('adds nothing when the workspace has no knowledge base', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true, {
      context: () => Promise.reject(new WikiRemoteError({ code: 'workspace_not_found', message: 'no knowledge base found' })),
    }))

    await expect(wiki.context({ task: 'anything' })).resolves.toBeUndefined()
  })

  it('propagates a knowledge base that failed for any other reason', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true, {
      context: () => Promise.reject(new WikiTransportError('bridge died', 'BRIDGE_EMPTY_RESPONSE')),
    }))

    await expect(wiki.context({ task: 'anything' })).rejects.toThrow(expect.objectContaining({ code: 'BRIDGE_EMPTY_RESPONSE' }))
  })

  it('passes only the budgets the caller set', async () => {
    const wiki = await mountWiki()
    let seen: unknown
    wiki.registerProvider(provider('athenaeum', true, {
      context: (request) => {
        seen = request
        return contextAnswer([])()
      },
    }))

    await wiki.context({ task: 'fix the OAuth callback', depth: 3 })

    expect(seen).toEqual({ method: 'context', task: 'fix the OAuth callback', depth: 3 })
  })
})

describe('utility reporting', () => {
  it('reports the nodes it was given', async () => {
    const wiki = await mountWiki()
    let seen: unknown
    wiki.registerProvider(provider('athenaeum', true, {
      record_use: (request) => {
        seen = request
        return Promise.resolve({ method: 'record_use', nodes: request.nodes })
      },
    }))

    await wiki.recordUse(['n1', 'n2'], 'sess_1')

    expect(seen).toEqual({ method: 'record_use', nodes: ['n1', 'n2'], session: 'sess_1' })
  })

  it('does not call the library with an empty report', async () => {
    const wiki = await mountWiki()
    const only = provider('athenaeum', true)
    wiki.registerProvider(only)

    await wiki.recordUse([], 'sess_1')

    expect(only.calls).toEqual([])
  })

  it('treats a workspace with no knowledge base as nothing to journal', async () => {
    const wiki = await mountWiki()
    wiki.registerProvider(provider('athenaeum', true, {
      record_use: () => Promise.reject(new WikiRemoteError({ code: 'workspace_not_found', message: 'no knowledge base found' })),
    }))

    await expect(wiki.recordUse(['n1'])).resolves.toBeUndefined()
  })
})
