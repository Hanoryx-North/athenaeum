import { describe, expect, it } from 'vitest'
import {
  BRIDGE_CORRELATION_MISMATCH,
  BRIDGE_EMPTY_RESPONSE,
  BRIDGE_INVALID_RESPONSE,
  BRIDGE_METHOD_MISMATCH,
  BRIDGE_PROTOCOL_MISMATCH,
  WikiBridgeClient,
  WikiRemoteError,
  WikiTransportError,
} from '../src/protocol/index.ts'
import { okEnvelope, recordingTransport } from './support.ts'

/** A client whose request ids are predictable, so correlation is testable. */
function client(answer: Parameters<typeof recordingTransport>[0]): {
  client: WikiBridgeClient
  transport: ReturnType<typeof recordingTransport>
} {
  const transport = recordingTransport(answer)
  return { client: new WikiBridgeClient({ transport, requestId: () => 'req-1' }), transport }
}

describe('WikiBridgeClient', () => {
  it('sends a versioned, correlated envelope and returns the response payload', async () => {
    const { client: wiki, transport } = client(envelope => okEnvelope(envelope, { method: 'search', hits: [] }))

    const response = await wiki.request({ method: 'search', query: 'OAuth callback', limit: 20 })

    expect(response.hits).toEqual([])
    expect(JSON.parse(transport.payloads[0] ?? '')).toEqual({
      protocolVersion: 1,
      requestId: 'req-1',
      request: { method: 'search', query: 'OAuth callback', limit: 20 },
    })
  })

  it('keeps additive response fields, as protocol v1 requires of consumers', async () => {
    const { client: wiki } = client(envelope => okEnvelope(envelope, {
      method: 'record_use',
      nodes: ['Decision_042'],
      recordedAt: '2026-08-21T00:00:00Z',
    }))

    const response = await wiki.request({ method: 'record_use', nodes: ['Decision_042'], session: 'sess_1' })

    expect(response.nodes).toEqual(['Decision_042'])
    expect(response).toHaveProperty('recordedAt', '2026-08-21T00:00:00Z')
  })

  it('raises the library error as a remote error carrying its code and details', async () => {
    const { client: wiki } = client(envelope => JSON.stringify({
      protocolVersion: 1,
      requestId: envelope['requestId'],
      status: 'error',
      error: {
        code: 'invalid_request',
        message: 'the title "auth_service" is a near-duplicate of existing node(s) [node_auth]',
        details: { title: 'auth_service', existingNodes: ['node_auth'] },
      },
    }))

    const failure = await wiki.request({ method: 'open', node: 'auth_service' }).catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(WikiRemoteError)
    const remote = failure as WikiRemoteError
    expect(remote.code).toBe('invalid_request')
    expect(remote.existingNodes()).toEqual(['node_auth'])
    expect(remote.isWorkspaceMissing).toBe(false)
  })

  it('marks a workspace without a knowledge base so the adapter can stand down', async () => {
    const { client: wiki } = client(envelope => JSON.stringify({
      protocolVersion: 1,
      requestId: envelope['requestId'],
      status: 'error',
      error: { code: 'workspace_not_found', message: 'no knowledge base found from /project' },
    }))

    const failure = await wiki.request({ method: 'context', task: 'anything' }).catch((error: unknown) => error)

    expect((failure as WikiRemoteError).isWorkspaceMissing).toBe(true)
  })

  it('refuses a response that answers a different request', async () => {
    const { client: wiki } = client(() => JSON.stringify({
      protocolVersion: 1,
      requestId: 'someone-elses-request',
      status: 'ok',
      response: { method: 'search', hits: [] },
    }))

    const failure = await wiki.request({ method: 'search', query: 'oauth' }).catch((error: unknown) => error)

    expect(failure).toBeInstanceOf(WikiTransportError)
    expect((failure as WikiTransportError).code).toBe(BRIDGE_CORRELATION_MISMATCH)
  })

  it('refuses a response that answers a different method', async () => {
    const { client: wiki } = client(envelope => okEnvelope(envelope, { method: 'navigate', nodes: [] }))

    const failure = await wiki.request({ method: 'search', query: 'oauth' }).catch((error: unknown) => error)

    expect((failure as WikiTransportError).code).toBe(BRIDGE_METHOD_MISMATCH)
  })

  it('refuses a protocol version it does not speak', async () => {
    const { client: wiki } = client(envelope => JSON.stringify({
      protocolVersion: 2,
      requestId: envelope['requestId'],
      status: 'ok',
      response: { method: 'capabilities', capabilities: {} },
    }))

    const failure = await wiki.capabilities().catch((error: unknown) => error)

    expect((failure as WikiTransportError).code).toBe(BRIDGE_PROTOCOL_MISMATCH)
  })

  it('reports the bridge exit status and diagnostics when it produced no response', async () => {
    const { client: wiki } = client(() => ({ stdout: '', stderr: 'athenaeum: command failed', exitCode: 127 }))

    const failure = await wiki.capabilities().catch((error: unknown) => error)

    expect((failure as WikiTransportError).code).toBe(BRIDGE_EMPTY_RESPONSE)
    expect((failure as Error).message).toContain('exit code 127')
    expect((failure as Error).message).toContain('athenaeum: command failed')
  })

  it('refuses output that is not a response envelope', async () => {
    const { client: wiki } = client(() => 'Usage: athenaeum [OPTIONS]')

    const failure = await wiki.capabilities().catch((error: unknown) => error)

    expect((failure as WikiTransportError).code).toBe(BRIDGE_INVALID_RESPONSE)
  })

  it('refuses an error envelope with no usable code', async () => {
    const { client: wiki } = client(envelope => JSON.stringify({
      protocolVersion: 1,
      requestId: envelope['requestId'],
      status: 'error',
      error: { message: 'something went wrong' },
    }))

    const failure = await wiki.capabilities().catch((error: unknown) => error)

    expect((failure as WikiTransportError).code).toBe(BRIDGE_INVALID_RESPONSE)
  })
})
