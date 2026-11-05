/**
 * Athenaeum Wiki protocol v1, as a plain TypeScript client.
 *
 * This module depends on Node and nothing else — no Cordis, no harness packages — so the
 * wire contract can be exercised against the library's published fixtures without a running
 * agent. Everything harness-shaped lives above it in `athenaeum-agent/wiki`.
 *
 * @module athenaeum-agent/protocol
 */

export { WikiBridgeClient } from './client.ts'
export type { WikiBridgeClientOptions } from './client.ts'
export {
  BRIDGE_CANCELLED,
  BRIDGE_CORRELATION_MISMATCH,
  BRIDGE_EMPTY_RESPONSE,
  BRIDGE_INVALID_RESPONSE,
  BRIDGE_METHOD_MISMATCH,
  BRIDGE_PROTOCOL_MISMATCH,
  BRIDGE_RESPONSE_TOO_LARGE,
  BRIDGE_SPAWN_FAILED,
  BRIDGE_TIMEOUT,
  BRIDGE_UNSUPPORTED_OPERATION,
  WikiBridgeError,
  WikiRemoteError,
  WikiTransportError,
} from './errors.ts'
export { bridgeArgs, spawnBridgeTransport } from './transport.ts'
export type { BridgeExchange, BridgeTransport, SpawnBridgeOptions } from './transport.ts'
export {
  existingNodesFromError,
  WIKI_ERROR_CODES,
  WIKI_INDEX_SCOPES,
  WIKI_METHODS,
  WIKI_PROTOCOL_VERSION,
  WIKI_REVIEW_KINDS,
  WIKI_REVIEW_SEVERITIES,
  WIKI_SOURCE_SCOPES,
  WORKSPACE_NOT_FOUND,
} from './types.ts'
export type {
  WikiBrief,
  WikiBriefNode,
  WikiBriefRelation,
  WikiCaller,
  WikiCapabilities,
  WikiContext,
  WikiEdge,
  WikiErrorCode,
  WikiEvidence,
  WikiEvidenceCreate,
  WikiGraphPath,
  WikiIndexReport,
  WikiIndexScope,
  WikiLimits,
  WikiLinkCreate,
  WikiMethod,
  WikiNode,
  WikiNodeCreate,
  WikiNodeDetail,
  WikiNodeUpdate,
  WikiNodeUtility,
  WikiNodeVersion,
  WikiPage,
  WikiProtocolErrorBody,
  WikiProvenance,
  WikiReached,
  WikiRelatedNode,
  WikiRequest,
  WikiRequestEnvelope,
  WikiRequestFor,
  WikiResponse,
  WikiResponseEnvelope,
  WikiResponseMap,
  WikiReviewFinding,
  WikiReviewSeverity,
  WikiSearchHit,
  WikiSourceAnchor,
  WikiSourceRecord,
  WikiSourceScope,
  WikiStatus,
  WikiStep,
} from './types.ts'
