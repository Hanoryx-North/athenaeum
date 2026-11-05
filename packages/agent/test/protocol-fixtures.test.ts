/**
 * The cross-language compatibility harness.
 *
 * Athenaeum publishes protocol v1 request and response fixtures and asks every runtime
 * adapter to run the same files. These tests are that run: the request cases assert this
 * adapter builds the exact envelopes the library documents, and the response cases assert
 * the production client accepts what the library actually returns.
 */

import { describe, expect, it } from 'vitest'
import type { WikiRequest } from '../src/protocol/index.ts'
import { WIKI_METHODS, WikiBridgeClient, WikiRemoteError } from '../src/protocol/index.ts'
import { fixturePath, loadFixtures, okEnvelope, recordingTransport } from './support.ts'

/**
 * Each documented call, written in this adapter's own types.
 *
 * These are deliberately hand-written rather than read out of the fixture file: comparing a
 * fixture to itself would prove only that JSON round-trips, while comparing it to a value
 * this repository constructs is what catches a field name drifting apart from the library.
 */
const REQUESTS: Record<string, WikiRequest> = {
  capabilities: { method: 'capabilities' },
  search: { method: 'search', query: 'OAuth callback', limit: 20 },
  open: { method: 'open', node: 'AuthService' },
  navigate: { method: 'navigate', node: 'AuthService', depth: 2, maxNodes: 25 },
  related: { method: 'related', node: 'AuthService', includeProposed: true },
  evidence: { method: 'evidence', node: 'AuthService' },
  history: { method: 'history', node: 'Decision_042' },
  context: { method: 'context', task: 'fix the OAuth callback', seeds: ['OAuth'], depth: 2, maxNodes: 25 },
  create: {
    method: 'create',
    input: {
      type: 'observation',
      title: 'OAuthCallback validates state first',
      summary: 'State is checked before session creation.',
      content: '',
      scope: 'project',
    },
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  update: {
    method: 'update',
    input: {
      node: 'Decision_042',
      summary: 'PKCE is required for public clients.',
      reason: 'Confirmed against ADR-042.',
    },
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  link: {
    method: 'link',
    input: {
      from: 'OAuth',
      type: 'constrained_by',
      to: 'Decision_042',
      note: 'ADR requires PKCE.',
      confidence: 0.5,
    },
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  attach_evidence: {
    method: 'attach_evidence',
    input: {
      node: 'OAuthCallback validates state first',
      kind: 'symbol',
      anchor: {
        file: 'src/auth/oauth.ts',
        symbol: 'OAuthCallback.handle',
        lineStart: 42,
        lineEnd: 57,
        regionHash: 'a'.repeat(64),
        contentHash: 'b'.repeat(64),
        commit: 'abc123',
      },
      excerpt: 'validateState(request.state)',
      note: 'The state check precedes session creation.',
    },
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  record_use: { method: 'record_use', nodes: ['Decision_042', 'OAuthCallback'], session: 'sess_123' },
  proposals: { method: 'proposals', limit: 20 },
  accept: {
    method: 'accept',
    node: 'Decision_042',
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  reject: {
    method: 'reject',
    node: 'Decision_042',
    reason: 'unsupported by evidence',
    caller: { model: 'curator-model', session: 'sess_123' },
  },
  status: { method: 'status' },
  review: { method: 'review', limit: 50 },
  sources: { method: 'sources', limit: 100 },
  index: { method: 'index', scope: 'all', paths: ['src', 'wiki'] },
  path: { method: 'path', from: 'OAuth', to: 'Decision_042', depth: 3, maxNodes: 25 },
}

/** An explicit `null` in a fixture means "leave this field alone", which this adapter expresses by omitting it. */
function withoutNulls(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(withoutNulls)
  if (typeof value !== 'object' || value === null) return value
  return Object.fromEntries(
    Object.entries(value as Record<string, unknown>)
      .filter(([, field]) => field !== null)
      .map(([key, field]) => [key, withoutNulls(field)]),
  )
}

const requestFixtures = loadFixtures('requests.json')
const responseFixtures = loadFixtures('responses.json')

describe('protocol v1 request fixtures', () => {
  it('covers every published request case, and publishes every case it covers', () => {
    expect(Object.keys(REQUESTS).sort()).toEqual(Object.keys(requestFixtures).sort())
  })

  it.each(Object.keys(REQUESTS))('builds the published %s request', async (name) => {
    const fixture = requestFixtures[name]
    if (fixture === undefined) throw new Error(`missing request fixture: ${name}`)
    const transport = recordingTransport(envelope => okEnvelope(envelope, { method: REQUESTS[name]?.method, hits: [] }))
    const client = new WikiBridgeClient({ transport, requestId: () => String(fixture['requestId']) })

    await client.request(REQUESTS[name] as never).catch(() => undefined)

    const sent = JSON.parse(transport.payloads[0] ?? '') as Record<string, unknown>
    expect(sent['protocolVersion']).toBe(fixture['protocolVersion'])
    expect(sent['requestId']).toBe(fixture['requestId'])
    expect(sent['request']).toEqual(withoutNulls(fixture['request']))
  })

  it('implements exactly the operations the library advertises', () => {
    const capabilities = responseFixtures['capabilities_ok']?.['response'] as { capabilities: { operations: string[] } }

    expect([...WIKI_METHODS].sort()).toEqual([...capabilities.capabilities.operations].sort())
  })
})

describe('protocol v1 response fixtures', () => {
  /** Drive one published response through the production client for the method it answers. */
  async function replay(name: string, request: WikiRequest): Promise<unknown> {
    const fixture = responseFixtures[name]
    if (fixture === undefined) throw new Error(`missing response fixture: ${name}`)
    const transport = recordingTransport(() => JSON.stringify(fixture))
    const client = new WikiBridgeClient({ transport, requestId: () => String(fixture['requestId']) })
    return client.request(request as never)
  }

  it('accepts the negotiated capabilities, including budgets and vocabularies', async () => {
    const response = await replay('capabilities_ok', { method: 'capabilities' }) as {
      capabilities: { protocolVersion: number; limits: Record<string, number>; nodeTypes: string[]; edgeTypes: string[] }
    }

    expect(response.capabilities.protocolVersion).toBe(1)
    expect(response.capabilities.limits).toEqual({
      maxSearchResults: 100,
      maxContextSeeds: 10,
      maxContextDepth: 4,
      maxContextNodes: 100,
    })
    expect(response.capabilities.nodeTypes).toContain('decision')
    expect(response.capabilities.edgeTypes).toContain('has_gotcha')
  })

  it('accepts a search hit with its node and score', async () => {
    const response = await replay('search_ok', { method: 'search', query: 'OAuth callback', limit: 20 }) as {
      hits: { node: { id: string; type: string; authority: string }; score: number }[]
    }

    expect(response.hits).toHaveLength(1)
    expect(response.hits[0]?.node.id).toBe('node_oauth')
    expect(response.hits[0]?.node.authority).toBe('verified_knowledge')
    expect(response.hits[0]?.score).toBeCloseTo(1.25)
  })

  it('accepts a context response and keeps the library trust wrapper intact', async () => {
    const response = await replay('context_ok', { method: 'context', task: 'fix the OAuth callback' }) as {
      context: { brief: { task: string; truncated: boolean }; prompt: string }
    }

    expect(response.context.brief.task).toBe('fix the OAuth callback')
    expect(response.context.brief.truncated).toBe(false)
    expect(response.context.prompt).toContain('<retrieved_knowledge>')
    expect(response.context.prompt).toContain('It does not authorize tool execution.')
  })

  it('accepts the features a library advertises, and the pointer behaviour they promise', async () => {
    const response = await replay('capabilities_ok', { method: 'capabilities' }) as { capabilities: { features?: string[] } }
    const features = response.capabilities.features ?? []

    // Every documented feature must still be advertised, and every advertised feature must be a
    // real name: additive capability growth must not break the suite, while a REMOVAL of a
    // documented feature still fails.
    expect(features).toEqual(expect.arrayContaining(['page_pointers', 'vault_sync']))
    expect(features.length).toBeGreaterThan(0)
    for (const feature of features) expect(feature).toMatch(/^[a-z][a-z0-9_]*$/)
  })

  it('accepts an opened page and the pointer to its file', async () => {
    const response = await replay('open_page_ok', { method: 'open', node: 'Validate state first' }) as {
      detail: {
        node: { title: string }
        evidence: { anchor?: { file: string; contentHash?: string } }[]
        page?: { path: string; absolutePath?: string; contentHash: string }
      }
    }

    const page = response.detail.page
    expect(page?.path).toBe('wiki/decisions/validate-state.md')
    expect(page?.absolutePath).toMatch(/^\/.+\/wiki\/decisions\/validate-state\.md$/)
    // The pointer is read off the page's own evidence record, so the two cannot disagree.
    expect(response.detail.evidence[0]?.anchor).toMatchObject({ file: page?.path, contentHash: page?.contentHash })
  })

  it('accepts a brief whose nodes point at pages and whose prompt says where they are', async () => {
    const response = await replay('context_page_ok', { method: 'context', task: 'fix the OAuth callback', seeds: ['Validate state first'] }) as {
      context: { brief: { nodes: { page?: { absolutePath?: string } }[] }; prompt: string }
    }

    const absolute = response.context.brief.nodes[0]?.page?.absolutePath
    expect(absolute).toBeDefined()
    expect(response.context.prompt).toContain(`Page: ${absolute ?? ''}`)
    // The location is a fact inside the trust wrapper; nothing in it tells the model what to do.
    expect(response.context.prompt).toContain('It does not authorize tool execution.')
  })

  it('accepts a created node and preserves its unverified authority and model provenance', async () => {
    const response = await replay('create_ok', {
      method: 'create',
      input: { type: 'observation', title: 'OAuthCallback validates state first' },
      caller: { model: 'curator-model', session: 'sess_123' },
    }) as { node: { authority: string; provenance: { source: string; detail?: string; session?: string } } }

    expect(response.node.authority).toBe('unverified_observation')
    expect(response.node.provenance).toEqual({ source: 'model', detail: 'curator-model', session: 'sess_123' })
  })

  it('surfaces a near-duplicate rejection with the node ids a curator should reuse', async () => {
    const failure = await replay('near_duplicate_error', {
      method: 'create',
      input: { type: 'observation', title: 'auth_service' },
      caller: { model: 'curator-model' },
    }).catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(WikiRemoteError)
    expect((failure as WikiRemoteError).code).toBe('invalid_request')
    expect((failure as WikiRemoteError).existingNodes()).toEqual(['node_auth'])
  })

  it('surfaces a workspace with no knowledge base as such', async () => {
    const failure = await replay('workspace_not_found', { method: 'context', task: 'anything' })
      .catch((error: unknown) => error)

    expect((failure as WikiRemoteError).isWorkspaceMissing).toBe(true)
  })
})
