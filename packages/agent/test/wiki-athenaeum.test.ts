import { describe, expect, it } from 'vitest'
import { AthenaeumWikiProvider, Config } from '../src/wiki-athenaeum/index.ts'
import type { BridgeExchange, BridgeTransport } from '../src/protocol/index.ts'
import { BRIDGE_PROTOCOL_MISMATCH, BRIDGE_UNSUPPORTED_OPERATION } from '../src/protocol/index.ts'

const CAPABILITIES = {
  protocolVersion: 1,
  operations: ['capabilities', 'search', 'context'],
  limits: { maxSearchResults: 100, maxContextSeeds: 10, maxContextDepth: 4, maxContextNodes: 100 },
  nodeTypes: [],
  edgeTypes: [],
  authorities: [],
  scopes: [],
  evidenceKinds: [],
}

/** A transport that answers each method from a script and counts the exchanges it served. */
function scriptedTransport(answer: (method: string, requestId: unknown) => unknown): BridgeTransport & { exchanges: string[] } {
  const exchanges: string[] = []
  return {
    exchanges,
    send(payload: string): Promise<BridgeExchange> {
      const envelope = JSON.parse(payload) as { requestId: string; request: { method: string } }
      exchanges.push(envelope.request.method)
      return Promise.resolve({
        stdout: JSON.stringify(answer(envelope.request.method, envelope.requestId)),
        stderr: '',
        exitCode: 0,
      })
    },
  }
}

/** A provider over a scripted transport, addressing a workspace that exists. */
function provider(transport: BridgeTransport, workspace = process.cwd()): AthenaeumWikiProvider {
  return new AthenaeumWikiProvider({
    id: 'athenaeum',
    command: 'athenaeum',
    workspace,
    timeoutMs: 1000,
    maxResponseBytes: 1024 * 1024,
    transport,
  })
}

/** A successful envelope for one method. */
function ok(requestId: unknown, response: unknown): unknown {
  return { protocolVersion: 1, requestId, status: 'ok', response }
}

describe('AthenaeumWikiProvider', () => {
  it('negotiates capabilities once and reuses them across calls', async () => {
    const transport = scriptedTransport((method, requestId) => ok(requestId, method === 'capabilities'
      ? { method: 'capabilities', capabilities: CAPABILITIES }
      : { method: 'search', hits: [] }))
    const wiki = provider(transport)

    await wiki.execute({ method: 'search', query: 'oauth' })
    await wiki.execute({ method: 'search', query: 'session' })

    expect(transport.exchanges).toEqual(['capabilities', 'search', 'search'])
  })

  it('answers a capabilities request from the negotiated result', async () => {
    const transport = scriptedTransport((_method, requestId) => ok(requestId, { method: 'capabilities', capabilities: CAPABILITIES }))
    const wiki = provider(transport)

    await wiki.capabilities()
    const response = await wiki.execute({ method: 'capabilities' })

    expect(response.capabilities.operations).toContain('search')
    expect(transport.exchanges).toEqual(['capabilities'])
  })

  it('refuses an operation the library does not advertise, naming it', async () => {
    const transport = scriptedTransport((_method, requestId) => ok(requestId, { method: 'capabilities', capabilities: CAPABILITIES }))
    const wiki = provider(transport)

    const failure = await wiki.execute({ method: 'history', node: 'AuthService' }).catch((error: unknown) => error)

    expect((failure as Error & { code: string }).code).toBe(BRIDGE_UNSUPPORTED_OPERATION)
    expect((failure as Error).message).toContain('"history"')
    expect(transport.exchanges).toEqual(['capabilities'])
  })

  it('refuses a library that speaks another protocol version', async () => {
    const transport = scriptedTransport((_method, requestId) => ok(requestId, {
      method: 'capabilities',
      capabilities: { ...CAPABILITIES, protocolVersion: 2 },
    }))

    const failure = await provider(transport).capabilities().catch((error: unknown) => error)

    expect((failure as Error & { code: string }).code).toBe(BRIDGE_PROTOCOL_MISMATCH)
  })

  it('retries a failed negotiation, so a knowledge base initialized later starts working', async () => {
    let attempt = 0
    const transport = scriptedTransport((_method, requestId) => {
      attempt += 1
      return attempt === 1
        ? { protocolVersion: 1, requestId, status: 'error', error: { code: 'workspace_not_found', message: 'no knowledge base found' } }
        : ok(requestId, { method: 'capabilities', capabilities: CAPABILITIES })
    })
    const wiki = provider(transport)

    await expect(wiki.capabilities()).rejects.toThrow(expect.objectContaining({ code: 'workspace_not_found' }))
    await expect(wiki.capabilities()).resolves.toMatchObject({ protocolVersion: 1 })
  })

  it('reports itself unusable for a workspace that does not exist', () => {
    const transport = scriptedTransport((_method, requestId) => ok(requestId, { method: 'capabilities', capabilities: CAPABILITIES }))

    expect(provider(transport, '/no/such/project').available()).toBe(false)
    expect(provider(transport).available()).toBe(true)
  })
})

describe('configuration', () => {
  it('fills in the documented defaults', () => {
    expect(Config({})).toMatchObject({
      id: 'athenaeum',
      command: 'athenaeum',
      timeoutMs: 30_000,
      maxResponseBytes: 4 * 1024 * 1024,
    })
  })

  it('refuses budgets that cannot bound anything', () => {
    // A zero or fractional budget bounds nothing, and a plugin that accepted one would
    // fail later, deep in a call, instead of at load where an operator can see it.
    expect(() => Config({ timeoutMs: 0 })).toThrow()
    expect(() => Config({ timeoutMs: 1.5 })).toThrow()
    expect(() => Config({ maxResponseBytes: 0 })).toThrow()
  })
})
