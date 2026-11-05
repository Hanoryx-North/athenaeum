/**
 * The tools as a mounted plugin: registration, argument validation, execution against the
 * seam, and what the model reads back.
 */

import { describe, expect, it } from 'vitest'
import { Context } from '@deepseek-ai/cordis'
import ToolRuntime from '@deepseek-ai/dsh-tools'
import type { ToolDefinition, ToolRunContext } from '@deepseek-ai/dsh-tools'
import SystemPrompt from '@deepseek-ai/dsh-system-prompt'
import WikiRuntime from '../src/wiki/index.ts'
import type { WikiProvider } from '../src/wiki/index.ts'
import type { WikiCapabilities, WikiMethod, WikiRequestFor, WikiResponseMap } from '../src/protocol/index.ts'
import * as toolWiki from '../src/tool-wiki/index.ts'

/** A tool's JSON result. The harness keeps this type in `dsh-util-values`, which this bundle does not depend on. */
type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue }

const CAPABILITIES: WikiCapabilities = {
  protocolVersion: 1,
  operations: ['capabilities', 'search', 'open', 'navigate', 'related', 'evidence', 'history', 'context', 'create', 'link', 'attach_evidence', 'record_use'],
  limits: { maxSearchResults: 100, maxContextSeeds: 10, maxContextDepth: 4, maxContextNodes: 100 },
  nodeTypes: [], edgeTypes: [], authorities: [], scopes: [], evidenceKinds: [],
}

/** A provider that records what it was asked and answers from a script. */
function provider(answers: Partial<{ [M in WikiMethod]: (request: WikiRequestFor<M>) => WikiResponseMap[M] }>): WikiProvider & { requests: unknown[] } {
  const requests: unknown[] = []
  return {
    id: 'athenaeum',
    requests,
    available: () => true,
    capabilities: () => Promise.resolve(CAPABILITIES),
    execute<M extends WikiMethod>(request: WikiRequestFor<M>): Promise<WikiResponseMap[M]> {
      requests.push(request)
      const answer = answers[request.method] as ((r: WikiRequestFor<M>) => WikiResponseMap[M]) | undefined
      if (answer === undefined) return Promise.reject(new Error(`unscripted ${request.method}`))
      return Promise.resolve(answer(request))
    },
  }
}

/** Mount the tool suite over a scripted provider. */
async function mountTools(
  config: toolWiki.Config,
  answers: Parameters<typeof provider>[0] = {},
): Promise<{ ctx: Context; requests: unknown[] }> {
  const ctx = new Context()
  await ctx.plugin(SystemPrompt, {})
  await ctx.plugin(ToolRuntime, {})
  await ctx.plugin(WikiRuntime, {})
  const scripted = provider(answers)
  ctx.wiki.registerProvider(scripted)
  await ctx.plugin(toolWiki, toolWiki.Config(config))
  return { ctx, requests: scripted.requests }
}

/** A minimal run context; the registry supplies the real one in production. */
function runContext(name: string, args: unknown): ToolRunContext {
  return {
    callId: 'call-1',
    rootCallId: 'call-1',
    token: 'token-1',
    name,
    arguments: args,
    signal: new AbortController().signal,
    deferContext: () => {},
    concludeTurn: () => {},
  } as unknown as ToolRunContext
}

/** Execute one registered tool and render what the model reads. */
async function call(ctx: Context, name: string, args: Record<string, unknown>): Promise<{ value: unknown; text: string }> {
  const tool = ctx.tools.get(name)
  if (tool === undefined) throw new Error(`tool not registered: ${name}`)
  const definition: ToolDefinition = tool
  // A tool's declared result is `unknown`; every value these tools return is JSON, and the
  // registry validates that in production.
  const value = await definition.execute(args, runContext(name, args)) as JsonValue
  const blocks = definition.output.render(args, value)
  return { value, text: blocks.map(block => block.type === 'text' ? block.text : '').join('\n') }
}

const NODE = {
  id: 'D_042',
  type: 'decision',
  title: 'Validate state before creating a session',
  summary: 'State is validated first.',
  content: '',
  scope: 'project',
  authority: 'project_decision',
  status: 'valid',
  provenance: { source: 'user' },
  createdAt: '2026-08-21T00:00:00Z',
  updatedAt: '2026-08-21T00:00:00Z',
  version: 1,
}

describe('tool registration', () => {
  it('registers the read tools and no curation tool by default', async () => {
    const { ctx } = await mountTools({})

    const registered = ctx.tools.schemas().map(schema => schema.name)
    expect(registered).toEqual(expect.arrayContaining([
      'wiki_search', 'wiki_open', 'wiki_navigate', 'wiki_related', 'wiki_evidence', 'wiki_history',
    ]))
    expect(registered).not.toContain('wiki_create')
  })

  it('registers the curation tools when a deployment turns them on', async () => {
    const { ctx } = await mountTools({ curation: true, curatorModel: 'curator-model' })

    const registered = ctx.tools.schemas().map(schema => schema.name)
    expect(registered).toEqual(expect.arrayContaining(['wiki_create', 'wiki_link', 'wiki_attach_evidence']))
  })

  it('refuses curation with no model to attribute proposals to', async () => {
    await expect(mountTools({ curation: true })).rejects.toThrow(/curatorModel/)
  })

  it('refuses a per-call budget that cannot bound anything', async () => {
    await expect(mountTools({ timeoutMs: 0 })).rejects.toThrow()
    await expect(mountTools({ timeoutMs: 1.5 })).rejects.toThrow()
  })

  it('registers nothing when both halves are off', async () => {
    const { ctx } = await mountTools({ read: false })

    expect(ctx.tools.schemas().map(schema => schema.name).filter(name => name.startsWith('wiki_'))).toEqual([])
  })
})

describe('read tools', () => {
  it('searches and renders hits the model can navigate from', async () => {
    const { ctx, requests } = await mountTools({}, {
      search: () => ({ method: 'search', hits: [{ node: NODE, score: 1.25 }] }),
    })

    const { value, text } = await call(ctx, 'wiki_search', { query: 'oauth callback', limit: 5 })

    expect(requests).toEqual([{ method: 'search', query: 'oauth callback', limit: 5 }])
    expect(value).toEqual({ hits: [{ id: 'D_042', title: NODE.title, type: 'decision', summary: NODE.summary, authority: 'project_decision', status: 'valid', score: 1.25 }] })
    expect(text).toContain('[D_042]')
    expect(text).toContain('project_decision')
  })

  it('says plainly when nothing matched, and why that is not the end of the search', async () => {
    const { ctx } = await mountTools({}, { search: () => ({ method: 'search', hits: [] }) })

    const { text } = await call(ctx, 'wiki_search', { query: 'nothing' })

    expect(text).toContain('relationships')
  })

  it('omits budgets the model did not set', async () => {
    const { ctx, requests } = await mountTools({}, { navigate: () => ({ method: 'navigate', nodes: [] }) })

    await call(ctx, 'wiki_navigate', { node: 'D_042' })

    expect(requests).toEqual([{ method: 'navigate', node: 'D_042' }])
  })

  it('shows how a walk reached each node', async () => {
    const { ctx } = await mountTools({}, {
      navigate: () => ({ method: 'navigate', nodes: [{ node: NODE, depth: 1, via: { from: 'OAuthCallback', edgeType: 'constrained_by' } }] }),
    })

    const { text } = await call(ctx, 'wiki_navigate', { node: 'OAuthCallback', depth: 2, max_nodes: 10 })

    expect(text).toContain('depth 1 via OAuthCallback -constrained_by->')
  })

  it('marks a relationship that is only proposed', async () => {
    const { ctx } = await mountTools({}, {
      related: () => ({
        method: 'related',
        relationships: [{ id: 'n2', title: 'Session-before-validation', type: 'gotcha', edgeType: 'has_gotcha', edgeId: 'e1', outbound: true, established: false }],
      }),
    })

    const { text } = await call(ctx, 'wiki_related', { node: 'D_042', include_proposed: true })

    expect(text).toContain('(proposed, not traversed)')
  })

  it('opens a node with its evidence, history and utility', async () => {
    const { ctx } = await mountTools({}, {
      open: () => ({
        method: 'open',
        detail: {
          node: NODE,
          relationships: [],
          evidence: [{ id: 'e1', node: 'D_042', kind: 'symbol', anchor: { file: 'src/auth/oauth.ts', symbol: 'OAuthCallback.handle', lineStart: 42, lineEnd: 57 }, excerpt: 'validateState(request.state)', createdAt: '2026-08-21T00:00:00Z' }],
          history: [{ node: 'D_042', version: 1, title: 'Earlier title', summary: 'Earlier claim.', content: '', authority: 'project_decision', status: 'valid', reason: 'Confirmed against ADR-042.', recordedAt: '2026-08-20T00:00:00Z' }],
          utility: { retrievals: 7, uses: 3, citations: 1, verifications: 0, rejections: 0 },
        },
      }),
    })

    const { text } = await call(ctx, 'wiki_open', { node: 'D_042' })

    expect(text).toContain('src/auth/oauth.ts:OAuthCallback.handle:42-57')
    expect(text).toContain('Confirmed against ADR-042.')
    expect(text).toContain('Retrieved 7 times, useful in 3.')
  })

  describe('a node that is a page of the vault', () => {
    const UTILITY = { retrievals: 0, uses: 0, citations: 0, verifications: 0, rejections: 0 }
    const HASH = 'b8163dbf245f627f1cbd019ad7997f2f0be69a97c0a1f95022fc1032ed96c910'

    function opening(page?: { path: string; absolutePath?: string; contentHash: string }) {
      return {
        open: (): WikiResponseMap['open'] => ({
          method: 'open',
          detail: { node: NODE, relationships: [], evidence: [], history: [], utility: UTILITY, ...page === undefined ? {} : { page } },
        }),
      }
    }

    it('names the file, and keeps the digest out of what the model reads', async () => {
      const { ctx } = await mountTools({}, opening({
        path: 'wiki/decisions/validate-state.md',
        absolutePath: '/work/project/wiki/decisions/validate-state.md',
        contentHash: HASH,
      }))

      const { value, text } = await call(ctx, 'wiki_open', { node: 'D_042' })

      expect(value).toMatchObject({
        page: { path: 'wiki/decisions/validate-state.md', absolutePath: '/work/project/wiki/decisions/validate-state.md' },
      })
      expect(JSON.stringify(value)).not.toContain(HASH)
      expect(text).toContain('Page: /work/project/wiki/decisions/validate-state.md')
    })

    it('falls back to the relative path when the library could not say where the project is', async () => {
      const { ctx } = await mountTools({}, opening({ path: 'wiki/decisions/validate-state.md', contentHash: HASH }))

      const { value, text } = await call(ctx, 'wiki_open', { node: 'D_042' })

      expect(text).toContain('Page: wiki/decisions/validate-state.md')
      expect((value as { page: Record<string, unknown> }).page).not.toHaveProperty('absolutePath')
    })

    it('shows nothing for a node that is not a page, and never invents a path', async () => {
      const { ctx } = await mountTools({}, opening())

      const { value, text } = await call(ctx, 'wiki_open', { node: 'D_042' })

      expect(value).not.toHaveProperty('page')
      expect(text).not.toContain('Page:')
    })

    it('tells the model the node and the file do different jobs, and not to move the file', async () => {
      const { ctx } = await mountTools({})

      const description = ctx.tools.schemas().find(schema => schema.name === 'wiki_open')?.description ?? ''

      expect(description).toContain('edit that file with your file tools')
      expect(description).toContain('do not move or rename it')
      expect(description).toContain('relationships, evidence, history')
    })
  })

  it('rejects arguments the schema does not accept', async () => {
    const { ctx } = await mountTools({}, { search: () => ({ method: 'search', hits: [] }) })

    await expect(call(ctx, 'wiki_search', { limit: 5 })).rejects.toThrow()
  })
})

describe('curation tools', () => {
  it('attributes a created node to the configured curator model', async () => {
    const { ctx, requests } = await mountTools({ curation: true, curatorModel: 'curator-model' }, {
      create: () => ({ method: 'create', node: { ...NODE, authority: 'unverified_observation' } }),
    })

    const { value } = await call(ctx, 'wiki_create', { type: 'observation', title: NODE.title, summary: NODE.summary })

    expect(requests).toEqual([{
      method: 'create',
      input: { type: 'observation', title: NODE.title, summary: NODE.summary },
      caller: { model: 'curator-model' },
    }])
    expect(value).toMatchObject({ authority: 'unverified_observation' })
  })

  it('turns a near-duplicate rejection into the ids a curator should reuse', async () => {
    const { WikiRemoteError } = await import('../src/protocol/index.ts')
    const { ctx } = await mountTools({ curation: true, curatorModel: 'curator-model' }, {
      create: () => {
        throw new WikiRemoteError({
          code: 'invalid_request',
          message: 'the title "auth_service" is a near-duplicate of existing node(s) [node_auth]',
          details: { existingNodes: ['node_auth'] },
        })
      },
    })

    await expect(call(ctx, 'wiki_create', { type: 'observation', title: 'auth_service', summary: 'x' }))
      .rejects.toThrow(/Use one of these existing nodes instead: node_auth/)
  })

  it('builds an evidence anchor only from the fields the model supplied', async () => {
    const { ctx, requests } = await mountTools({ curation: true, curatorModel: 'curator-model' }, {
      attach_evidence: () => ({ method: 'attach_evidence', evidence: { id: 'e1', node: 'D_042', kind: 'symbol', createdAt: '2026-08-21T00:00:00Z' } }),
    })

    await call(ctx, 'wiki_attach_evidence', { node: 'D_042', kind: 'symbol', file: 'src/auth/oauth.ts', symbol: 'OAuthCallback.handle' })

    expect(requests).toEqual([{
      method: 'attach_evidence',
      input: { node: 'D_042', kind: 'symbol', anchor: { file: 'src/auth/oauth.ts', symbol: 'OAuthCallback.handle' } },
      caller: { model: 'curator-model' },
    }])
  })
})
