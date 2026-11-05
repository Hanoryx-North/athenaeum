/**
 * The Athenaeum knowledge provider: Wiki protocol v1 over the `athenaeum bridge` command.
 *
 * The provider knows the protocol and nothing about how Athenaeum stores knowledge — not
 * SQLite, not FTS5, not content-addressed blobs. That is the boundary the library asks
 * consumers to respect, and it is what lets the storage change without touching this
 * runtime.
 *
 * @module athenaeum-agent/wiki-athenaeum/provider
 */

import { existsSync } from 'node:fs'
import {
  BRIDGE_PROTOCOL_MISMATCH,
  BRIDGE_UNSUPPORTED_OPERATION,
  WIKI_PROTOCOL_VERSION,
  WikiBridgeClient,
  WikiTransportError,
  spawnBridgeTransport,
} from '../protocol/index.ts'
import type { BridgeTransport, WikiCapabilities, WikiMethod, WikiRequestFor, WikiResponseMap } from '../protocol/index.ts'
import type { WikiProvider } from '../wiki/index.ts'

/** How to reach one Athenaeum knowledge base. */
export interface AthenaeumProviderOptions {
  /** Seam registry id. */
  readonly id: string
  /** The Athenaeum executable; a bare name resolves through `PATH`. */
  readonly command: string
  /** The project directory whose knowledge base is addressed. */
  readonly workspace: string
  /** Wall-clock budget for one bridge call, in milliseconds. */
  readonly timeoutMs: number
  /** Cap on bytes read from one bridge response. */
  readonly maxResponseBytes: number
  /** Transport override. Defaults to spawning the command once per request, as protocol v1 specifies. */
  readonly transport?: BridgeTransport
}

/**
 * A knowledge provider backed by an Athenaeum checkout.
 *
 * Capabilities are negotiated once and reused, because protocol v1 spawns a process per
 * call and re-negotiating before each one would double that cost. A failed negotiation is
 * not cached: an executable that appears later, or a knowledge base initialized mid-session,
 * must be able to start working without restarting the agent.
 */
export class AthenaeumWikiProvider implements WikiProvider {
  readonly id: string
  private readonly options: AthenaeumProviderOptions
  private readonly client: WikiBridgeClient
  private negotiated: Promise<WikiCapabilities> | undefined

  constructor(options: AthenaeumProviderOptions) {
    this.id = options.id
    this.options = options
    this.client = new WikiBridgeClient({
      transport: options.transport ?? spawnBridgeTransport({
        command: options.command,
        workspace: options.workspace,
        timeoutMs: options.timeoutMs,
        maxResponseBytes: options.maxResponseBytes,
      }),
    })
  }

  /**
   * Whether this provider is worth calling.
   *
   * The check is local and cheap by contract, so it answers only what can be known without
   * running anything: a named executable and a directory that exists. Whether that directory
   * holds a knowledge base is the library's answer to give, as `workspace_not_found`.
   */
  available(): boolean {
    return this.options.command.length > 0 && existsSync(this.options.workspace)
  }

  /**
   * Negotiate what this library build serves, reusing the first successful answer.
   *
   * @param signal - optional cancellation forwarded to the transport.
   * @returns the advertised capabilities.
   * @throws {WikiTransportError} `BRIDGE_PROTOCOL_MISMATCH` when the library speaks another protocol version.
   */
  async capabilities(signal?: AbortSignal): Promise<WikiCapabilities> {
    const pending = this.negotiated ?? this.negotiate(signal)
    this.negotiated = pending
    try {
      return await pending
    } catch (error: unknown) {
      if (this.negotiated === pending) this.negotiated = undefined
      throw error
    }
  }

  /**
   * Run one operation, after confirming the library advertises it.
   *
   * Checking against `capabilities` turns "this build is older than this adapter" into one
   * clear failure naming the operation, instead of an `invalid_request` from deep inside the
   * library.
   *
   * @param request - the operation and its arguments.
   * @param signal - optional cancellation forwarded to the transport.
   * @returns the response payload for that method.
   */
  async execute<M extends WikiMethod>(request: WikiRequestFor<M>, signal?: AbortSignal): Promise<WikiResponseMap[M]> {
    const capabilities = await this.capabilities(signal)
    if (request.method === 'capabilities') {
      return { method: 'capabilities', capabilities } as WikiResponseMap[M]
    }
    if (!capabilities.operations.includes(request.method)) {
      throw new WikiTransportError(
        `the knowledge library at ${this.options.workspace} does not serve the "${request.method}" operation`,
        BRIDGE_UNSUPPORTED_OPERATION,
      )
    }
    return this.client.request(request, signal)
  }

  /** Ask the library what it serves and refuse a version this adapter cannot speak. */
  private async negotiate(signal?: AbortSignal): Promise<WikiCapabilities> {
    const capabilities = await this.client.capabilities(signal)
    if (capabilities.protocolVersion !== WIKI_PROTOCOL_VERSION) {
      throw new WikiTransportError(
        `the knowledge library at ${this.options.workspace} serves protocol version ${capabilities.protocolVersion}; this adapter speaks version ${WIKI_PROTOCOL_VERSION}`,
        BRIDGE_PROTOCOL_MISMATCH,
      )
    }
    return capabilities
  }
}
