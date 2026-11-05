/** Stable v1 JSON contract for the one-shot `athenaeum bridge` process. */
export const BRIDGE_PROTOCOL_VERSION = 1 as const

export const BRIDGE_METHODS = [
  'capabilities', 'search', 'open', 'navigate', 'related', 'evidence', 'history',
  'context', 'create', 'update', 'link', 'attach_evidence', 'record_use',
  'proposals', 'accept', 'reject', 'status', 'review', 'sources', 'index', 'path',
] as const
export type BridgeMethod = typeof BRIDGE_METHODS[number]

/** Why a review finding was raised. A closed wire vocabulary, not a knowledge vocabulary. */
export const BRIDGE_REVIEW_KINDS = [
  'contradiction', 'dispute', 'stale_anchor', 'unsupported_rule', 'unreviewed_proposal',
  'duplicate', 'near_duplicate', 'superseded', 'incomplete_scan', 'orphan',
] as const
export type BridgeReviewKind = typeof BRIDGE_REVIEW_KINDS[number]

/** How much a review finding should worry a reader before it worries a build. */
export const BRIDGE_REVIEW_SEVERITIES = ['blocking', 'warning', 'info'] as const
export type BridgeReviewSeverity = typeof BRIDGE_REVIEW_SEVERITIES[number]

/** Which scan read a source: the vault pass or the repository pass. Never `all`. */
export const BRIDGE_SOURCE_SCOPES = ['vault', 'repository'] as const
export type BridgeSourceScope = typeof BRIDGE_SOURCE_SCOPES[number]

/** What `index` rescans: the vault, the repository, or both. */
export const BRIDGE_INDEX_SCOPES = ['vault', 'repository', 'all'] as const
export type BridgeIndexScope = typeof BRIDGE_INDEX_SCOPES[number]

/** Closed knowledge vocabularies come from `capabilities`, never from this package. */
export interface BridgeCaller { model: string; session?: string }
export interface BridgeNodeCreate { type: string; title: string; summary?: string; content?: string; scope?: string }
export interface BridgeNodeUpdate { node: string; title?: string | null; summary?: string | null; content?: string | null; reason?: string | null }
export interface BridgeLinkCreate { from: string; type: string; to: string; note?: string | null; confidence?: number | null }
export interface BridgeSourceAnchor { file: string; symbol?: string; lineStart?: number; lineEnd?: number; regionHash?: string; contentHash?: string; commit?: string }
export interface BridgeEvidenceCreate { node: string; kind?: string; anchor?: BridgeSourceAnchor; excerpt?: string; note?: string }

export type BridgeRequest =
  | { method: 'capabilities' }
  | { method: 'search'; query: string; limit?: number }
  | { method: 'open'; node: string }
  | { method: 'navigate'; node: string; depth?: number; maxNodes?: number }
  | { method: 'related'; node: string; includeProposed?: boolean }
  | { method: 'evidence'; node: string }
  | { method: 'history'; node: string }
  | { method: 'context'; task: string; seeds?: string[]; depth?: number; maxNodes?: number }
  | { method: 'create'; input: BridgeNodeCreate; caller: BridgeCaller }
  | { method: 'update'; input: BridgeNodeUpdate; caller: BridgeCaller }
  | { method: 'link'; input: BridgeLinkCreate; caller: BridgeCaller }
  | { method: 'attach_evidence'; input: BridgeEvidenceCreate; caller: BridgeCaller }
  | { method: 'record_use'; nodes: string[]; session?: string }
  | { method: 'proposals'; limit?: number }
  | { method: 'accept'; node: string; caller: BridgeCaller }
  | { method: 'reject'; node: string; reason?: string; caller: BridgeCaller }
  | { method: 'status' }
  | { method: 'review'; limit?: number }
  | { method: 'sources'; limit?: number }
  | { method: 'index'; scope: BridgeIndexScope; paths?: string[] }
  | { method: 'path'; from: string; to: string; depth?: number; maxNodes?: number }

export interface BridgeRequestEnvelope<R extends BridgeRequest = BridgeRequest> {
  protocolVersion: typeof BRIDGE_PROTOCOL_VERSION
  requestId?: string
  request: R
}

export interface BridgeLimits { maxSearchResults: number; maxContextSeeds: number; maxContextDepth: number; maxContextNodes: number }
export interface BridgeCapabilities {
  protocolVersion: number
  operations: string[]
  limits: BridgeLimits
  nodeTypes: string[]
  edgeTypes: string[]
  authorities: string[]
  scopes: string[]
  evidenceKinds: string[]
  features?: string[]
}

export interface BridgeProvenance { source: string; detail?: string; session?: string }
export interface BridgeNode {
  id: string; type: string; title: string; summary: string; content: string
  scope: string; authority: string; status: string; provenance: BridgeProvenance
  lastVerifiedAt?: string; createdAt: string; updatedAt: string; version: number
}
export interface BridgeEdge {
  id: string; from: string; to: string; type: string; provenance: BridgeProvenance
  status: string; scope: string; confidence?: number; note?: string; createdAt: string
}
export interface BridgeEvidence {
  id: string; node: string; kind: string; anchor?: BridgeSourceAnchor
  blobHash?: string; excerpt?: string; note?: string; createdAt: string
}
export interface BridgeNodeVersion {
  node: string; version: number; title: string; summary: string; content: string
  authority: string; status: string; reason?: string; recordedAt: string
}
export interface BridgeNodeUtility { retrievals: number; uses: number; citations: number; verifications: number; rejections: number }
/**
 * A relationship as a caller sees it. `note` and `confidence` mirror the fields of `BridgeEdge`
 * and are omitted when the store has neither; a proposed relationship keeps `established: false`.
 */
export interface BridgeRelatedNode {
  id: string; title: string; type: string; edgeType: string; edgeId: string; outbound: boolean
  established: boolean; note?: string; confidence?: number
}
export interface BridgePage { path: string; absolutePath?: string; contentHash: string }
export interface BridgeNodeDetail {
  node: BridgeNode; relationships: BridgeRelatedNode[]; evidence: BridgeEvidence[]
  history: BridgeNodeVersion[]; utility: BridgeNodeUtility; page?: BridgePage
}
export interface BridgeBriefNode { id: string; title: string; type: string; summary: string; authority: string; status: string; depth: number; page?: BridgePage }
export interface BridgeBrief {
  task: string; nodes: BridgeBriefNode[]; relationships: { from: string; type: string; to: string }[]
  anchors: string[]; warnings: string[]; truncated: boolean
}
/** Inject `prompt` verbatim; it contains the reference-material trust wrapper. */
export interface BridgeContext { brief: BridgeBrief; prompt: string }
export interface BridgeProposal { node: BridgeNode; relationships: BridgeRelatedNode[]; evidence: BridgeEvidence[] }

/** One row of the review queue: what needs a human decision, and what it rests on. */
export interface BridgeReviewFinding {
  id: string; kind: BridgeReviewKind; severity: BridgeReviewSeverity; title: string; detail: string
  nodes: string[]; evidence: string[]; sourcePath: string | null; confidence: number; blocking: boolean
}
/**
 * One file one scan read, with the coverage that scan actually has of it. `path` is project
 * relative. There is one record per (scope, path) pair, so a file both scans read appears twice
 * and `scope` says why; `scanned` means the named scan read it, and `sha256` is then the real
 * content hash. `indexedAt` is when a scan recorded the row — present even when `scanned` is
 * false, because the scan looked and chose not to read. The list is not restricted to
 * `raw/sources/**`.
 */
export interface BridgeSourceRecord {
  scope: BridgeSourceScope; path: string; bytes: number; sha256: string; scanned: boolean
  nodes: number; anchors: number; indexedAt: string | null; stale: boolean
}
/** What the last `index` run covered, and what it could not. */
export interface BridgeIndexReport {
  vaultPages: number; repositoryFiles: number; symbols: number; edges: number
  durationMs: number; incomplete: boolean; skipped: string[]
}
/**
 * The workspace as a caller and a human see it before choosing an operation.
 *
 * `indexedAt: null` carries "never indexed", and `indexStale` says an index exists but the
 * sources have moved on since it was built; a workspace with no index yet reports
 * `indexStale: false`, never `true`.
 */
export interface BridgeStatus {
  workspacePath: string; initialized: boolean; hasVault: boolean
  vaultPages: number; nodes: number; edges: number; proposals: number; findings: number
  indexedAt: string | null; indexStale: boolean; protocolVersion: number; engineVersion: string
}
/** One hop of a connection: the node it starts from, the edge type it crosses, and that edge's note / confidence when it has them. */
export interface BridgePathStep { from: string; edgeType: string; note?: string; confidence?: number }
/**
 * How `from` connects to `to`. `found: false` means no path exists within the budget; it is a
 * normal answer, not an error, and `steps`/`nodes` are then empty.
 */
export interface BridgeGraphPath {
  from: string; to: string; found: boolean; steps: BridgePathStep[]; nodes: BridgeNode[]
}

export interface BridgeResponseMap {
  capabilities: { method: 'capabilities'; capabilities: BridgeCapabilities }
  search: { method: 'search'; hits: { node: BridgeNode; score: number }[] }
  open: { method: 'open'; detail: BridgeNodeDetail }
  navigate: { method: 'navigate'; nodes: { node: BridgeNode; depth: number; via?: { from: string; edgeType: string; note?: string; confidence?: number } }[] }
  related: { method: 'related'; relationships: BridgeRelatedNode[] }
  evidence: { method: 'evidence'; evidence: BridgeEvidence[] }
  history: { method: 'history'; versions: BridgeNodeVersion[] }
  context: { method: 'context'; context: BridgeContext }
  create: { method: 'create'; node: BridgeNode }
  update: { method: 'update'; node: BridgeNode }
  link: { method: 'link'; edge: BridgeEdge }
  attach_evidence: { method: 'attach_evidence'; evidence: BridgeEvidence }
  record_use: { method: 'record_use'; nodes: string[] }
  proposals: { method: 'proposals'; proposals: BridgeProposal[] }
  accept: { method: 'accept'; accepted: { node: BridgeNode; page: BridgePage; alreadyAccepted: boolean; relationshipsWritten: number; relationshipsLeftLocal: string[] } }
  reject: { method: 'reject'; node: BridgeNode }
  status: { method: 'status'; status: BridgeStatus }
  review: { method: 'review'; findings: BridgeReviewFinding[] }
  sources: { method: 'sources'; sources: BridgeSourceRecord[] }
  index: { method: 'index'; index: BridgeIndexReport }
  path: { method: 'path'; path: BridgeGraphPath }
}
export type BridgeResponse = BridgeResponseMap[BridgeMethod]
export interface ProtocolError { code: string; message: string; details?: Record<string, unknown> }
export type BridgeResponseEnvelope =
  | { protocolVersion: 1; requestId?: string; status: 'ok'; response: BridgeResponse }
  | { protocolVersion: 1; requestId?: string; status: 'error'; error: ProtocolError }

export const BRIDGE_ERROR_CODES = [
  'invalid_request', 'not_found', 'incompatible_store', 'knowledge_error',
  'request_too_large', 'unsupported_protocol', 'workspace_not_found',
] as const

export function bridgeRequest<R extends BridgeRequest>(request: R, requestId?: string): BridgeRequestEnvelope<R> {
  return { protocolVersion: BRIDGE_PROTOCOL_VERSION, ...(requestId ? { requestId } : {}), request }
}

export function existingNodesFromError(error: ProtocolError): string[] {
  const value = error.details?.existingNodes
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : []
}
