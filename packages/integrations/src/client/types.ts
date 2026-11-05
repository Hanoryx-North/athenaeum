/**
 * Wire shapes of bridge protocol v1 and of the sync commands that this package relies on. The bridge
 * shapes are the ones `@athenaeum/protocol` publishes; the sync shapes mirror the JSON that
 * `athenaeum sync ... --json` prints.
 *
 * The additive v1 surfaces (`status`, `review`, `sources`, `index`, `path`) mirror the same
 * contract: snake_case method names, camelCase payloads, no fields invented here. A client DTO
 * below never adds meaning; it only names what the wire carries and bounds what a tool may return.
 */

import {
  BRIDGE_INDEX_SCOPES,
  BRIDGE_PROTOCOL_VERSION,
  BRIDGE_REVIEW_KINDS,
  BRIDGE_REVIEW_SEVERITIES,
  type BridgeCapabilities,
  type BridgeCaller,
  type BridgeContext,
  type BridgeEdge,
  type BridgeEvidence,
  type BridgeGraphPath,
  type BridgeIndexReport,
  type BridgeIndexScope,
  type BridgeNode,
  type BridgeNodeCreate,
  type BridgeNodeDetail,
  type BridgeNodeVersion,
  type BridgePage,
  type BridgePathStep,
  type BridgeProposal,
  type BridgeRelatedNode,
  type BridgeResponseMap,
  type BridgeReviewFinding,
  type BridgeReviewKind,
  type BridgeReviewSeverity,
  type BridgeSourceRecord,
  type BridgeStatus,
} from '@athenaeum/protocol/bridge';

export const PROTOCOL_VERSION = BRIDGE_PROTOCOL_VERSION;

export type Caller = BridgeCaller;
export type KnowledgeNode = BridgeNode;
export type PageRef = BridgePage;
export type RelatedNode = BridgeRelatedNode;
export type NodeDetail = BridgeNodeDetail;
/** `prompt` is the brief already wrapped in the engine's trust boundary; hand it over verbatim. */
export type Context = BridgeContext;
export type Proposal = BridgeProposal;
export type Accepted = BridgeResponseMap['accept']['accepted'];
export type NewNode = BridgeNodeCreate;
export type Capabilities = BridgeCapabilities;
export type Evidence = BridgeEvidence;
export type Edge = BridgeEdge;
export type NodeVersion = BridgeNodeVersion;
export type NavigatedNode = BridgeResponseMap['navigate']['nodes'][number];

export interface SearchHit {
  node: KnowledgeNode;
  score: number;
}

/**
 * Why the engine raised a review finding. `@athenaeum/protocol` owns the closed vocabulary; this
 * package only names the values it may receive back.
 */
export const REVIEW_KINDS = BRIDGE_REVIEW_KINDS;
export type ReviewKind = BridgeReviewKind;

/** How much a finding should worry a reader before it worries a build. */
export const REVIEW_SEVERITIES = BRIDGE_REVIEW_SEVERITIES;
export type ReviewSeverity = BridgeReviewSeverity;

/** What `index` rescans: the vault, the repository, or both. */
export const INDEX_SCOPES = BRIDGE_INDEX_SCOPES;
export type IndexScope = BridgeIndexScope;

/** The workspace as a caller and a person see it before choosing an operation. */
export type EngineStatus = BridgeStatus;

/** One row of the review queue: what needs a human decision, and what it rests on. */
export type ReviewFinding = BridgeReviewFinding;

/** One indexed source file or vault page, with the coverage the index actually has of it. */
export type SourceRecord = BridgeSourceRecord;

/** What the last `index` run covered, and what it could not. */
export type IndexReport = BridgeIndexReport;

/** One hop of a graph path: `from` is the node it leaves, `edgeType` the edge it follows. */
export type PathStep = BridgePathStep;

/** `found: false` means no path exists within the walk's budget. That is an answer, not an error. */
export type KnowledgePath = BridgeGraphPath;

/** How much of a list a caller asks for. Every list is bounded; there is no unbounded read. */
export interface Bounds {
  limit?: number;
  offset?: number;
}

/**
 * A window over a list the engine holds. `hasMore` is derived from a real fetch of one more item
 * than the window holds, so it is never a guess; `total` is present only when the whole list was
 * actually read, and is never invented.
 */
export interface Page<T> {
  items: T[];
  offset: number;
  limit: number;
  hasMore: boolean;
  total?: number;
}

export interface ReviewFilters {
  kind?: ReviewKind;
  blocking?: boolean;
}

/**
 * A review window. When a filter is set the engine's list is read up to `scan` findings and then
 * filtered; `scanComplete` says whether the whole list was read, so a filtered page never pretends
 * to be exhaustive.
 */
export interface ReviewPage extends Page<ReviewFinding> {
  scanned: number;
  scanComplete: boolean;
  filters: ReviewFilters;
}

export interface ReviewQuery extends Bounds, ReviewFilters {
  /** How many findings may be read before filtering. The engine's own list may be longer. */
  scan?: number;
}

export interface SourceFilters {
  prefix?: string;
  staleOnly?: boolean;
}

export interface SourcePage extends Page<SourceRecord> {
  scanned: number;
  scanComplete: boolean;
  filters: SourceFilters;
}

export interface SourceQuery extends Bounds, SourceFilters {
  /** How many sources may be read before filtering. The engine's own list may be longer. */
  scan?: number;
}

export interface IndexRequest {
  scope: IndexScope;
  /** Restrict the scan to these repository paths. Omit to scan the whole scope. */
  paths?: string[];
}

/** `athenaeum repo info --json`. Works before `athenaeum init`. */
export interface RepoInfo {
  path: string;
  initialized: boolean;
  repositoryId: string | null;
  name: string | null;
  formatVersion: number | null;
  nodes: number | null;
  proposals: number | null;
  pages: number | null;
  remote: { server: string; remoteId: string; lastSynced: string | null; conflicts: number } | null;
}

export const SYNC_ACTIONS = ['status', 'conflicts', 'pull', 'push', 'sync', 'resolve', 'link', 'create'] as const;
/**
 * `link` and `create` are the one-time setup (`athenaeum sync link --remote ...` / `--create ...`);
 * `status`, `pull`, `push`, `resolve` are the everyday verbs; `sync` is pull then push.
 */
export type SyncAction = (typeof SYNC_ACTIONS)[number];

export type SyncPosition = 'not_linked' | 'offline' | 'conflicted' | 'up_to_date' | 'ahead' | 'behind' | 'diverged';
export interface Pending { added: string[]; modified: string[]; deleted: string[] }
/** `athenaeum sync status --json`. */
export interface SyncStatus {
  position: SyncPosition;
  server: string | null;
  remoteId: string | null;
  lastSynced: string | null;
  remoteHead: string | null;
  pending: Pending;
  conflicts: string[];
  skipped: { path: string; reason: string }[];
  reachable: boolean;
  detail: string | null;
}
export interface PullOutcome {
  upToDate: boolean;
  revision: string | null;
  updated: string[];
  deleted: string[];
  merged: string[];
  conflicts: string[];
  changed: boolean;
}
export interface PushOutcome { nothingToPush: boolean; revision: string | null; pushed: Pending }

export type Keep = 'local' | 'remote' | 'both';
export const KEEP_CHOICES = ['local', 'remote', 'both'] as const satisfies readonly Keep[];

export interface SyncResult {
  /** True when the command succeeded and left nothing waiting on a person. */
  ok: boolean;
  /** The command's exit status (a pull that stops on conflicts, and an offline push, exit 1). */
  exitCode: number;
  /** `offline` and `conflicted` leave the repository exactly as it was; both are things to report to a person. */
  state: 'done' | 'offline' | 'conflicted';
  /** The CLI's own JSON (`SyncStatus`, `{pull, index}`, `PushOutcome`, ...). */
  result: unknown;
}
