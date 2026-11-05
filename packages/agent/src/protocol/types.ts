/** Agent-facing, immutable names for the one shared wire contract.
 * Runtime semantics and transport stay in this adapter; shapes belong to protocol.
 * Closed knowledge vocabularies are supplied by the engine's capabilities.
 */
import * as protocol from '@athenaeum/protocol/bridge'

type Immutable<T> = T extends object ? { readonly [K in keyof T]: Immutable<T[K]> } : T

export type WikiCaller = Immutable<protocol.BridgeCaller>
export type WikiNodeCreate = Immutable<protocol.BridgeNodeCreate>
export type WikiNodeUpdate = Immutable<protocol.BridgeNodeUpdate>
export type WikiLinkCreate = Immutable<protocol.BridgeLinkCreate>
export type WikiSourceAnchor = Immutable<protocol.BridgeSourceAnchor>
export type WikiEvidenceCreate = Immutable<protocol.BridgeEvidenceCreate>
export type WikiRequest = Immutable<protocol.BridgeRequest>
export type WikiRequestEnvelope = Immutable<protocol.BridgeRequestEnvelope>
export type WikiLimits = Immutable<protocol.BridgeLimits>
export type WikiCapabilities = Immutable<protocol.BridgeCapabilities>
export type WikiProvenance = Immutable<protocol.BridgeProvenance>
export type WikiNode = Immutable<protocol.BridgeNode>
export type WikiEdge = Immutable<protocol.BridgeEdge>
export type WikiEvidence = Immutable<protocol.BridgeEvidence>
export type WikiNodeVersion = Immutable<protocol.BridgeNodeVersion>
export type WikiNodeUtility = Immutable<protocol.BridgeNodeUtility>
export type WikiRelatedNode = Immutable<protocol.BridgeRelatedNode>
export type WikiPage = Immutable<protocol.BridgePage>
export type WikiNodeDetail = Immutable<protocol.BridgeNodeDetail>
export type WikiBriefNode = Immutable<protocol.BridgeBriefNode>
export type WikiBrief = Immutable<protocol.BridgeBrief>
export type WikiContext = Immutable<protocol.BridgeContext>
export type WikiIndexScope = Immutable<protocol.BridgeIndexScope>
export type WikiSourceScope = Immutable<protocol.BridgeSourceScope>
export type WikiReviewSeverity = Immutable<protocol.BridgeReviewSeverity>
export type WikiStatus = Immutable<protocol.BridgeStatus>
export type WikiReviewFinding = Immutable<protocol.BridgeReviewFinding>
export type WikiSourceRecord = Immutable<protocol.BridgeSourceRecord>
export type WikiIndexReport = Immutable<protocol.BridgeIndexReport>
export type WikiGraphPath = Immutable<protocol.BridgeGraphPath>
export type WikiResponseMap = Immutable<protocol.BridgeResponseMap>
export type WikiResponse = Immutable<protocol.BridgeResponse>
export type WikiResponseEnvelope = Immutable<protocol.BridgeResponseEnvelope>

export type WikiMethod = protocol.BridgeMethod
export type WikiRequestFor<M extends WikiMethod> = Extract<WikiRequest, { method: M }>
export type WikiSearchHit = WikiResponseMap['search']['hits'][number]
export type WikiStep = Immutable<protocol.BridgePathStep>
export type WikiReached = WikiResponseMap['navigate']['nodes'][number]
export type WikiBriefRelation = WikiBrief['relationships'][number]
export type WikiProtocolErrorBody = Immutable<protocol.ProtocolError>
export type WikiErrorCode = (typeof protocol.BRIDGE_ERROR_CODES)[number]

export const WIKI_PROTOCOL_VERSION = protocol.BRIDGE_PROTOCOL_VERSION
export const WIKI_METHODS = protocol.BRIDGE_METHODS
export const WIKI_ERROR_CODES = protocol.BRIDGE_ERROR_CODES
export const WIKI_REVIEW_KINDS = protocol.BRIDGE_REVIEW_KINDS
export const WIKI_REVIEW_SEVERITIES = protocol.BRIDGE_REVIEW_SEVERITIES
export const WIKI_INDEX_SCOPES = protocol.BRIDGE_INDEX_SCOPES
export const WIKI_SOURCE_SCOPES = protocol.BRIDGE_SOURCE_SCOPES
export const WORKSPACE_NOT_FOUND = 'workspace_not_found'

export function existingNodesFromError(error: WikiProtocolErrorBody): readonly string[] {
  return protocol.existingNodesFromError(error)
}
