/**
 * Failures of the Wiki protocol bridge, split by who is responsible.
 *
 * A {@link WikiRemoteError} is the library answering "no" through the protocol: the request
 * reached it, and its `code` is one of the stable v1 codes. A {@link WikiTransportError} is
 * this side failing to obtain an answer at all — the command did not run, the call was
 * cancelled, or the bytes on stdout were not a v1 envelope.
 *
 * The distinction decides behaviour, not just wording. A remote error is knowledge the model
 * can act on; a transport error means the knowledge layer is unavailable and the task
 * continues without it.
 *
 * @module athenaeum-agent/protocol/errors
 */

import type { WikiProtocolErrorBody } from './types.ts'
import { existingNodesFromError, WORKSPACE_NOT_FOUND } from './types.ts'

/** Base for every bridge failure. `code` is stable and routable; never parse `message`. */
export class WikiBridgeError extends Error {
  /** Stable machine-routable failure class. */
  readonly code: string

  constructor(message: string, code: string, options?: ErrorOptions) {
    super(message, options)
    this.code = code
    this.name = new.target.name
  }
}

/**
 * The library rejected the request. `code` is a protocol v1 error code — one of
 * `invalid_request`, `not_found`, `incompatible_store`, `knowledge_error`,
 * `request_too_large`, `unsupported_protocol` or `workspace_not_found`, or a
 * provider-specific code a later version adds.
 */
export class WikiRemoteError extends WikiBridgeError {
  /** Additive machine-readable context, when the library attached any. */
  readonly details: Record<string, unknown> | undefined

  constructor(body: WikiProtocolErrorBody, options?: ErrorOptions) {
    super(body.message, body.code, options)
    this.details = body.details
  }

  /** True when this workspace has no knowledge base, so the adapter should behave as though no wiki were installed. */
  get isWorkspaceMissing(): boolean {
    return this.code === WORKSPACE_NOT_FOUND
  }

  /**
   * Node ids a near-duplicate rejection points at.
   * @returns the existing node ids, or an empty array when the error names none.
   */
  existingNodes(): readonly string[] {
    return existingNodesFromError({ code: this.code, message: this.message, ...this.details ? { details: this.details } : {} })
  }
}

/** Local failure codes. They are SCREAMING_SNAKE so they can never be confused with the protocol's snake_case codes. */
export const BRIDGE_SPAWN_FAILED = 'BRIDGE_SPAWN_FAILED'
/** The bridge exceeded its time budget and was killed. */
export const BRIDGE_TIMEOUT = 'BRIDGE_TIMEOUT'
/** The caller aborted before a response arrived. */
export const BRIDGE_CANCELLED = 'BRIDGE_CANCELLED'
/** The bridge wrote nothing to stdout. */
export const BRIDGE_EMPTY_RESPONSE = 'BRIDGE_EMPTY_RESPONSE'
/** The bridge wrote bytes that are not a v1 response envelope. */
export const BRIDGE_INVALID_RESPONSE = 'BRIDGE_INVALID_RESPONSE'
/** The response declared a protocol version this adapter does not speak. */
export const BRIDGE_PROTOCOL_MISMATCH = 'BRIDGE_PROTOCOL_MISMATCH'
/** The response carried a different `requestId` than the request. */
export const BRIDGE_CORRELATION_MISMATCH = 'BRIDGE_CORRELATION_MISMATCH'
/** A successful response answered a different method than the one asked. */
export const BRIDGE_METHOD_MISMATCH = 'BRIDGE_METHOD_MISMATCH'
/** The response exceeded the configured byte cap and the bridge was killed. */
export const BRIDGE_RESPONSE_TOO_LARGE = 'BRIDGE_RESPONSE_TOO_LARGE'
/** The negotiated capabilities do not advertise the requested operation. */
export const BRIDGE_UNSUPPORTED_OPERATION = 'BRIDGE_UNSUPPORTED_OPERATION'

/** No answer was obtained. The knowledge layer is unavailable for this call; the task proceeds without it. */
export class WikiTransportError extends WikiBridgeError {}
