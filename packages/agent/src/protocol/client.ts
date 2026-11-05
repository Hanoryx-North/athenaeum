/**
 * The Wiki protocol v1 client: one typed request in, one validated response out.
 *
 * Everything the wire contract promises is checked here — the declared protocol version,
 * the request-id correlation, and that a successful response answers the method that was
 * asked. A response failing any of those is a {@link WikiTransportError}, never a value
 * handed further up: a knowledge layer that answers a different question than it was asked
 * would otherwise reach a model as fact.
 *
 * Unknown response fields pass through untouched, as protocol v1 requires of consumers.
 *
 * @module athenaeum-agent/protocol/client
 */

import { randomUUID } from 'node:crypto'
import {
  BRIDGE_CORRELATION_MISMATCH,
  BRIDGE_EMPTY_RESPONSE,
  BRIDGE_INVALID_RESPONSE,
  BRIDGE_METHOD_MISMATCH,
  BRIDGE_PROTOCOL_MISMATCH,
  WikiRemoteError,
  WikiTransportError,
} from './errors.ts'
import type { BridgeTransport } from './transport.ts'
import type {
  WikiCapabilities,
  WikiMethod,
  WikiProtocolErrorBody,
  WikiRequestEnvelope,
  WikiRequestFor,
  WikiResponseMap,
} from './types.ts'
import { WIKI_PROTOCOL_VERSION } from './types.ts'

/**
 * Where each method's answer lives, and what shape it must have.
 *
 * A response envelope can be well-formed — right version, right request id, right method —
 * and still carry nothing useful, because a library bug or a proxy wrote `{}`. Reading the
 * payload key here turns that into an `BRIDGE_INVALID_RESPONSE` instead of a `TypeError` two
 * frames up that no consumer can route on. The `Record<WikiMethod, …>` annotation is the
 * point: a new protocol method cannot be added without saying where its answer is.
 */
const RESPONSE_PAYLOAD: Record<WikiMethod, { readonly key: string; readonly kind: 'object' | 'array' }> = {
  capabilities: { key: 'capabilities', kind: 'object' },
  search: { key: 'hits', kind: 'array' },
  open: { key: 'detail', kind: 'object' },
  navigate: { key: 'nodes', kind: 'array' },
  related: { key: 'relationships', kind: 'array' },
  evidence: { key: 'evidence', kind: 'array' },
  history: { key: 'versions', kind: 'array' },
  context: { key: 'context', kind: 'object' },
  create: { key: 'node', kind: 'object' },
  update: { key: 'node', kind: 'object' },
  link: { key: 'edge', kind: 'object' },
  attach_evidence: { key: 'evidence', kind: 'object' },
  record_use: { key: 'nodes', kind: 'array' },
  proposals: { key: 'proposals', kind: 'array' },
  accept: { key: 'accepted', kind: 'object' },
  reject: { key: 'node', kind: 'object' },
  status: { key: 'status', kind: 'object' },
  review: { key: 'findings', kind: 'array' },
  sources: { key: 'sources', kind: 'array' },
  index: { key: 'index', kind: 'object' },
  path: { key: 'path', kind: 'object' },
}

/** How the client is built. */
export interface WikiBridgeClientOptions {
  /** The exchange mechanism; `spawnBridgeTransport` is the one-shot process implementation. */
  readonly transport: BridgeTransport
  /** Correlation id source. Defaults to a random UUID per request; tests supply a deterministic one. */
  readonly requestId?: () => string
}

/** Describe a failed exchange for an error message, including the bridge's own diagnostics. */
function exchangeDetail(stderr: string, exitCode: number | null): string {
  const parts: string[] = []
  if (exitCode !== null && exitCode !== 0) parts.push(`exit code ${exitCode}`)
  const trimmed = stderr.trim()
  if (trimmed.length > 0) parts.push(`stderr: ${trimmed}`)
  return parts.length > 0 ? ` (${parts.join('; ')})` : ''
}

/** True when a parsed value could be a response envelope at all. */
function isEnvelopeLike(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/** Read the error body from an envelope that declared `status: "error"`. */
function errorBody(envelope: Record<string, unknown>): WikiProtocolErrorBody {
  const error = envelope['error']
  if (!isEnvelopeLike(error) || typeof error['code'] !== 'string' || typeof error['message'] !== 'string') {
    throw new WikiTransportError(
      'the wiki bridge reported an error without a usable code and message',
      BRIDGE_INVALID_RESPONSE,
    )
  }
  const details = error['details']
  return {
    code: error['code'],
    message: error['message'],
    ...isEnvelopeLike(details) ? { details } : {},
  }
}

/**
 * A typed client for one Wiki library.
 *
 * The client is stateless: it negotiates nothing and caches nothing, so a caller that needs
 * capability negotiation owns that decision. `WikiBridgeProvider` is the caller that does.
 */
export class WikiBridgeClient {
  private readonly transport: BridgeTransport
  private readonly nextRequestId: () => string

  constructor(options: WikiBridgeClientOptions) {
    this.transport = options.transport
    this.nextRequestId = options.requestId ?? randomUUID
  }

  /**
   * Run one request and return its response payload.
   *
   * @param request - the operation and its arguments.
   * @param signal - optional cancellation forwarded to the transport.
   * @returns the response payload for that method.
   * @throws {WikiRemoteError} when the library answered with a protocol error.
   * @throws {WikiTransportError} when no valid v1 response could be obtained.
   */
  async request<M extends WikiMethod>(request: WikiRequestFor<M>, signal?: AbortSignal): Promise<WikiResponseMap[M]> {
    const requestId = this.nextRequestId()
    const envelope: WikiRequestEnvelope = {
      protocolVersion: WIKI_PROTOCOL_VERSION,
      requestId,
      request,
    }
    const exchange = await this.transport.send(JSON.stringify(envelope), signal)
    const raw = exchange.stdout.trim()
    if (raw.length === 0) {
      throw new WikiTransportError(
        `the wiki bridge produced no response${exchangeDetail(exchange.stderr, exchange.exitCode)}`,
        BRIDGE_EMPTY_RESPONSE,
      )
    }
    let parsed: unknown
    try {
      parsed = JSON.parse(raw)
    } catch (error: unknown) {
      throw new WikiTransportError(
        `the wiki bridge wrote output that is not JSON${exchangeDetail(exchange.stderr, exchange.exitCode)}`,
        BRIDGE_INVALID_RESPONSE,
        { cause: error },
      )
    }
    if (!isEnvelopeLike(parsed)) {
      throw new WikiTransportError('the wiki bridge response is not an envelope object', BRIDGE_INVALID_RESPONSE)
    }
    if (parsed['protocolVersion'] !== WIKI_PROTOCOL_VERSION) {
      throw new WikiTransportError(
        `the wiki bridge answered protocol version ${JSON.stringify(parsed['protocolVersion'])}; this adapter speaks version ${WIKI_PROTOCOL_VERSION}`,
        BRIDGE_PROTOCOL_MISMATCH,
      )
    }
    const answeredId = parsed['requestId']
    if (answeredId !== undefined && answeredId !== requestId) {
      throw new WikiTransportError(
        `the wiki bridge answered request ${JSON.stringify(answeredId)} for request ${JSON.stringify(requestId)}`,
        BRIDGE_CORRELATION_MISMATCH,
      )
    }
    const status = parsed['status']
    if (status === 'error') throw new WikiRemoteError(errorBody(parsed))
    if (status !== 'ok') {
      throw new WikiTransportError(
        `the wiki bridge answered with an unknown status ${JSON.stringify(status)}`,
        BRIDGE_INVALID_RESPONSE,
      )
    }
    const response = parsed['response']
    if (!isEnvelopeLike(response)) {
      throw new WikiTransportError('the wiki bridge reported success without a response payload', BRIDGE_INVALID_RESPONSE)
    }
    if (response['method'] !== request.method) {
      throw new WikiTransportError(
        `asked the wiki bridge for ${request.method} and it answered ${JSON.stringify(response['method'])}`,
        BRIDGE_METHOD_MISMATCH,
      )
    }
    const payload = RESPONSE_PAYLOAD[request.method]
    const answer = response[payload.key]
    const usable = payload.kind === 'array' ? Array.isArray(answer) : isEnvelopeLike(answer)
    if (!usable) {
      throw new WikiTransportError(
        `the wiki bridge answered ${request.method} without a usable "${payload.key}" payload`,
        BRIDGE_INVALID_RESPONSE,
      )
    }
    return response as WikiResponseMap[M]
  }

  /**
   * Negotiate what this library build can do.
   *
   * The protocol requires this before anything else: it reports the exact protocol version,
   * the operations served, the closed vocabularies and the retrieval budgets, and it answers
   * even for a project that has never run `athenaeum init`.
   *
   * @param signal - optional cancellation forwarded to the transport.
   * @returns the advertised capabilities.
   */
  async capabilities(signal?: AbortSignal): Promise<WikiCapabilities> {
    const response = await this.request({ method: 'capabilities' }, signal)
    return response.capabilities
  }
}
