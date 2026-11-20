/**
 * The desktop's typed half of bridge protocol v1.
 *
 * The types are the protocol's own (`@athenaeum/protocol/bridge`), imported
 * type-only so this module stays free of runtime dependencies: it builds the
 * exact request envelopes the engine answers and validates the answers back
 * into protocol values. `src/lib/engine.ts` is what actually sends them
 * (through the `engine_request` Tauri command, which validates both halves
 * again in Rust); keeping the wire description here — pure functions, no
 * `@tauri-apps` import, no `@/` alias — is what makes the contract checkable
 * without launching the app.
 *
 * This request surface is limited to the knowledge UI's operations.
 *
 * If an answer does not match the protocol, parsing throws an `EngineError`
 * naming the field, and the UI shows a protocol-error state with a recovery
 * action rather than rendering a half-understood object as if it were
 * knowledge.
 */
import type {
  BridgeBrief,
  BridgeContext,
  BridgeEvidence,
  BridgeGraphPath,
  BridgeIndexReport,
  BridgeIndexScope,
  BridgeNode,
  BridgeNodeDetail,
  BridgeNodeVersion,
  BridgePage,
  BridgeProposal,
  BridgeRelatedNode,
  BridgeRequest,
  BridgeResponseMap,
  BridgeReviewFinding,
  BridgeReviewKind,
  BridgeReviewSeverity,
  BridgeSourceAnchor,
  BridgeSourceRecord,
  BridgeSourceScope,
} from "@athenaeum/protocol/bridge"

/** The protocol version this desktop speaks. */
export const ENGINE_PROTOCOL_VERSION = 1

/** Methods the knowledge UI may ask for. */
export type EngineMethod =
  | "status"
  | "review"
  | "sources"
  | "index"
  | "proposals"
  | "context"
  | "open"
  | "history"
  | "path"

// ---------------------------------------------------------------------------
// Types, taken from the protocol rather than written down again
// ---------------------------------------------------------------------------

/** One bridge request, narrowed to what this window is allowed to ask. */
export type EngineRequest = Extract<BridgeRequest, { method: EngineMethod }>

/** The answer to one of those requests, narrowed the same way. */
export type EngineResponse = BridgeResponseMap[EngineMethod]

export type EngineStatus = BridgeResponseMap["status"]["status"]
export type EngineFinding = BridgeReviewFinding
/** Which scan read a file: the vault pass or the repository pass. */
export type EngineSourceScope = BridgeSourceScope
/**
 * One record per (scan, path). A file read by the vault pass and skipped by
 * the repository pass is two records, and only one of them is scanned; the
 * pair is the identity, so nothing is ever collapsed by path.
 */
export type EngineSource = BridgeSourceRecord
export type EngineIndexReport = BridgeIndexReport
export type EngineProposal = BridgeProposal
export type EngineProposalNode = BridgeProposal["node"]
export type EngineRelationship = BridgeRelatedNode
export type EngineEvidence = BridgeEvidence
export type EngineAnchor = BridgeSourceAnchor
export type EngineVersion = BridgeNodeVersion
export type EnginePage = BridgePage
export type EngineNode = BridgeNode
export type EngineNodeDetail = BridgeNodeDetail
export type EngineBrief = BridgeBrief
export type EngineBriefNode = BridgeBrief["nodes"][number]
export type EngineContext = BridgeContext
export type EngineGraphPath = BridgeGraphPath
export type FindingSeverity = BridgeReviewSeverity
export type FindingKind = BridgeReviewKind
export type IndexScope = BridgeIndexScope

// ---------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------

export function statusRequest(): EngineRequest {
  return { method: "status" }
}

export function reviewRequest(limit?: number): EngineRequest {
  return limit === undefined ? { method: "review" } : { method: "review", limit }
}

export function sourcesRequest(limit?: number): EngineRequest {
  return limit === undefined ? { method: "sources" } : { method: "sources", limit }
}

/**
 * Reconcile the index. The scope is required by the protocol: the UI says
 * what it is asking to be rebuilt, and defaults to everything.
 */
export function indexRequest(options: { scope?: BridgeIndexScope; paths?: string[] } = {}): EngineRequest {
  return {
    method: "index",
    scope: options.scope ?? "all",
    ...(options.paths !== undefined ? { paths: [...options.paths] } : {}),
  }
}

export function proposalsRequest(limit?: number): EngineRequest {
  return limit === undefined ? { method: "proposals" } : { method: "proposals", limit }
}

/** A brief is bounded by the engine; these are the only knobs the UI offers. */
export function briefRequest(
  task: string,
  options: { depth?: number; maxNodes?: number } = {},
): EngineRequest {
  return {
    method: "context",
    task,
    ...(options.depth !== undefined ? { depth: options.depth } : {}),
    ...(options.maxNodes !== undefined ? { maxNodes: options.maxNodes } : {}),
  }
}

/** One node with its relationships, evidence, history and page. */
export function openRequest(node: string): EngineRequest {
  return { method: "open", node }
}

export function historyRequest(node: string): EngineRequest {
  return { method: "history", node }
}

/** How two nodes connect, according to the graph the engine holds. */
export function pathRequest(
  from: string,
  to: string,
  options: { depth?: number; maxNodes?: number } = {},
): EngineRequest {
  return {
    method: "path",
    from,
    to,
    ...(options.depth !== undefined ? { depth: options.depth } : {}),
    ...(options.maxNodes !== undefined ? { maxNodes: options.maxNodes } : {}),
  }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/**
 * Every error code the desktop can receive: the protocol's own closed set,
 * plus the two the shell adds at its boundary.
 */
export const ENGINE_ERROR_CODES = [
  "invalid_request",
  "not_found",
  "incompatible_store",
  "knowledge_error",
  "request_too_large",
  "unsupported_protocol",
  "workspace_not_found",
  "engine_protocol_error",
  "library_state_unreadable",
  "transport",
] as const

export type EngineErrorCode = (typeof ENGINE_ERROR_CODES)[number] | (string & {})

export class EngineError extends Error {
  readonly code: EngineErrorCode

  constructor(code: EngineErrorCode, message: string) {
    super(message)
    this.name = "EngineError"
    this.code = code
  }
}

function isKnownCode(code: string): boolean {
  return (ENGINE_ERROR_CODES as readonly string[]).includes(code)
}

/**
 * Normalize anything thrown by the shell into an `EngineError`.
 *
 * Tauri rejects a command with the plain string the Rust side returned, which
 * for this app is always `code: message`. Anything else — a missing IPC
 * bridge, a dropped window — is a transport failure.
 */
export function engineError(cause: unknown): EngineError {
  if (cause instanceof EngineError) return cause
  const raw =
    typeof cause === "string"
      ? cause
      : cause instanceof Error
        ? cause.message
        : String(cause ?? "")
  const separator = raw.indexOf(":")
  if (separator > 0) {
    const code = raw.slice(0, separator).trim()
    if (isKnownCode(code)) {
      return new EngineError(code, raw.slice(separator + 1).trim() || raw)
    }
  }
  return new EngineError("transport", raw || "the engine did not answer")
}

// ---------------------------------------------------------------------------
// The engine's own explanation of a proposal
// ---------------------------------------------------------------------------

/**
 * The proposal reason/confidence the review queue shows.
 *
 * v1 has no proposal-level confidence, so the queue reads the engine's own
 * `unreviewed_proposal` finding for the same node. If there is no such
 * finding, the queue says the engine stated neither, rather than guessing.
 */
export interface ProposalExplanation {
  reason: string | null
  confidence: number | null
  severity: FindingSeverity | null
  blocking: boolean
  finding: EngineFinding | null
}

export function explainProposal(
  node: string,
  findings: readonly EngineFinding[],
): ProposalExplanation {
  const finding =
    findings.find(
      (candidate) => candidate.kind === "unreviewed_proposal" && candidate.nodes.includes(node),
    ) ?? null
  return {
    reason: finding ? finding.detail : null,
    confidence: finding ? finding.confidence : null,
    severity: finding ? finding.severity : null,
    blocking: finding ? finding.blocking : false,
    finding,
  }
}

// ---------------------------------------------------------------------------
// The Library link, as the shell reports it
// ---------------------------------------------------------------------------

/**
 * Which Library repository this project is linked to, if any.
 *
 * Read from `athenaeum.json` and the sync state on disk by the shell. No
 * network call is made, so reachability is unknown rather than guessed.
 */
export interface LibraryLink {
  workspacePath: string
  manifestPresent: boolean
  repositoryId: string | null
  repositoryName: string | null
  linked: boolean
  server: string | null
  remoteId: string | null
  remoteName: string | null
  lastSynced: string | null
  lastSyncAt: string | null
  conflicts: string[]
}

// ---------------------------------------------------------------------------
// Boundary validation
// ---------------------------------------------------------------------------

function protocolError(path: string, expected: string, value: unknown): never {
  const seen =
    value === null
      ? "null"
      : Array.isArray(value)
        ? `an array of ${value.length}`
        : typeof value === "string"
          ? JSON.stringify(value.length > 48 ? `${value.slice(0, 48)}…` : value)
          : typeof value === "object"
            ? "an object"
            : String(value)
  throw new EngineError(
    "engine_protocol_error",
    `the engine's answer does not match protocol v${ENGINE_PROTOCOL_VERSION}: ${path} should be ${expected}, not ${seen}`,
  )
}

function recordAt(value: unknown, path: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    protocolError(path, "an object", value)
  }
  return value as Record<string, unknown>
}

/** A field the shape requires to be present. */
function fieldAt(record: Record<string, unknown>, key: string, path: string): unknown {
  if (!(key in record)) {
    protocolError(`${path}.${key}`, "present", undefined)
  }
  return record[key]
}

function textAt(value: unknown, path: string): string {
  if (typeof value !== "string") protocolError(path, "a string", value)
  return value
}

function numberAt(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) protocolError(path, "a number", value)
  return value
}

function booleanAt(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") protocolError(path, "a boolean", value)
  return value
}

function nullableTextAt(value: unknown, path: string): string | null {
  if (value === null) return null
  if (typeof value !== "string") protocolError(path, "a string or null", value)
  return value
}

function optionalTextAt(record: Record<string, unknown>, key: string, path: string): string | undefined {
  const value = record[key]
  if (value === undefined || value === null) return undefined
  return textAt(value, `${path}.${key}`)
}

function optionalNumberAt(record: Record<string, unknown>, key: string, path: string): number | undefined {
  const value = record[key]
  if (value === undefined || value === null) return undefined
  return numberAt(value, `${path}.${key}`)
}

function arrayAt(value: unknown, path: string): unknown[] {
  if (!Array.isArray(value)) protocolError(path, "an array", value)
  return value
}

function textArrayAt(value: unknown, path: string): string[] {
  return arrayAt(value, path).map((item, index) => textAt(item, `${path}[${index}]`))
}

function textArrayField(record: Record<string, unknown>, key: string, path: string): string[] {
  return textArrayAt(fieldAt(record, key, path), `${path}.${key}`)
}

function nullableFieldAt(record: Record<string, unknown>, key: string, path: string): string | null {
  return nullableTextAt(fieldAt(record, key, path), `${path}.${key}`)
}

function listField<T>(
  record: Record<string, unknown>,
  key: string,
  path: string,
  parse: (value: unknown, path: string) => T,
): T[] {
  return arrayAt(fieldAt(record, key, path), `${path}.${key}`).map((item, index) =>
    parse(item, `${path}.${key}[${index}]`),
  )
}

/**
 * A severity or kind the vocabulary does not know is kept verbatim: a newer
 * engine adding a kind must not blank the panel, and the UI labels what it
 * does not recognize as unrecognized rather than guessing a meaning.
 */
function severityAt(value: unknown, path: string): BridgeReviewSeverity {
  return textAt(value, path) as BridgeReviewSeverity
}

function kindAt(value: unknown, path: string): BridgeReviewKind {
  return textAt(value, path) as BridgeReviewKind
}

export function parseEngineStatus(value: unknown, path = "status"): EngineStatus {
  const record = recordAt(value, path)
  return {
    workspacePath: textAt(fieldAt(record, "workspacePath", path), `${path}.workspacePath`),
    initialized: booleanAt(fieldAt(record, "initialized", path), `${path}.initialized`),
    hasVault: booleanAt(fieldAt(record, "hasVault", path), `${path}.hasVault`),
    vaultPages: numberAt(fieldAt(record, "vaultPages", path), `${path}.vaultPages`),
    nodes: numberAt(fieldAt(record, "nodes", path), `${path}.nodes`),
    edges: numberAt(fieldAt(record, "edges", path), `${path}.edges`),
    proposals: numberAt(fieldAt(record, "proposals", path), `${path}.proposals`),
    findings: numberAt(fieldAt(record, "findings", path), `${path}.findings`),
    indexedAt: nullableTextAt(fieldAt(record, "indexedAt", path), `${path}.indexedAt`),
    indexStale: booleanAt(fieldAt(record, "indexStale", path), `${path}.indexStale`),
    protocolVersion: numberAt(fieldAt(record, "protocolVersion", path), `${path}.protocolVersion`),
    engineVersion: textAt(fieldAt(record, "engineVersion", path), `${path}.engineVersion`),
  }
}

export function parseEngineFinding(value: unknown, path = "finding"): EngineFinding {
  const record = recordAt(value, path)
  return {
    id: textAt(fieldAt(record, "id", path), `${path}.id`),
    kind: kindAt(fieldAt(record, "kind", path), `${path}.kind`),
    severity: severityAt(fieldAt(record, "severity", path), `${path}.severity`),
    title: textAt(fieldAt(record, "title", path), `${path}.title`),
    detail: textAt(fieldAt(record, "detail", path), `${path}.detail`),
    nodes: textArrayField(record, "nodes", path),
    evidence: textArrayField(record, "evidence", path),
    sourcePath: nullableFieldAt(record, "sourcePath", path),
    confidence: numberAt(fieldAt(record, "confidence", path), `${path}.confidence`),
    blocking: booleanAt(fieldAt(record, "blocking", path), `${path}.blocking`),
  }
}

/** A scope the vocabulary does not know is kept verbatim, so a newer engine
 * adding one does not blank the list; the UI labels it as it stands. */
function scopeAt(value: unknown, path: string): BridgeSourceScope {
  return textAt(value, path) as BridgeSourceScope
}

export function parseEngineSource(value: unknown, path = "source"): EngineSource {
  const record = recordAt(value, path)
  return {
    scope: scopeAt(fieldAt(record, "scope", path), `${path}.scope`),
    path: textAt(fieldAt(record, "path", path), `${path}.path`),
    bytes: numberAt(fieldAt(record, "bytes", path), `${path}.bytes`),
    sha256: textAt(fieldAt(record, "sha256", path), `${path}.sha256`),
    scanned: booleanAt(fieldAt(record, "scanned", path), `${path}.scanned`),
    nodes: numberAt(fieldAt(record, "nodes", path), `${path}.nodes`),
    anchors: numberAt(fieldAt(record, "anchors", path), `${path}.anchors`),
    indexedAt: nullableFieldAt(record, "indexedAt", path),
    stale: booleanAt(fieldAt(record, "stale", path), `${path}.stale`),
  }
}

export function parseEngineIndexReport(value: unknown, path = "index"): EngineIndexReport {
  const record = recordAt(value, path)
  return {
    vaultPages: numberAt(fieldAt(record, "vaultPages", path), `${path}.vaultPages`),
    repositoryFiles: numberAt(fieldAt(record, "repositoryFiles", path), `${path}.repositoryFiles`),
    symbols: numberAt(fieldAt(record, "symbols", path), `${path}.symbols`),
    edges: numberAt(fieldAt(record, "edges", path), `${path}.edges`),
    durationMs: numberAt(fieldAt(record, "durationMs", path), `${path}.durationMs`),
    incomplete: booleanAt(fieldAt(record, "incomplete", path), `${path}.incomplete`),
    skipped: textArrayField(record, "skipped", path),
  }
}

function parseProvenance(value: unknown, path: string) {
  const record = recordAt(value, path)
  const provenance: EngineNode["provenance"] = {
    source: textAt(fieldAt(record, "source", path), `${path}.source`),
  }
  const detail = optionalTextAt(record, "detail", path)
  if (detail !== undefined) provenance.detail = detail
  const session = optionalTextAt(record, "session", path)
  if (session !== undefined) provenance.session = session
  return provenance
}

function parseApplicability(record: Record<string, unknown>, path: string): EngineNode["applicability"] {
  if (record.applicability === undefined || record.applicability === null) return undefined
  const applicability = recordAt(record.applicability, `${path}.applicability`)
  return {
    ...(applicability.core === undefined ? {} : { core: booleanAt(applicability.core, `${path}.applicability.core`) }),
    ...(applicability.projects === undefined ? {} : { projects: textArrayAt(applicability.projects, `${path}.applicability.projects`) }),
    ...(applicability.technologies === undefined ? {} : { technologies: textArrayAt(applicability.technologies, `${path}.applicability.technologies`) }),
    ...(applicability.stages === undefined ? {} : { stages: textArrayAt(applicability.stages, `${path}.applicability.stages`) }),
  }
}

export function parseEngineNode(value: unknown, path = "node"): EngineNode {
  const record = recordAt(value, path)
  const node: EngineNode = {
    id: textAt(fieldAt(record, "id", path), `${path}.id`),
    type: textAt(fieldAt(record, "type", path), `${path}.type`),
    title: textAt(fieldAt(record, "title", path), `${path}.title`),
    summary: textAt(fieldAt(record, "summary", path), `${path}.summary`),
    content: textAt(fieldAt(record, "content", path), `${path}.content`),
    scope: textAt(fieldAt(record, "scope", path), `${path}.scope`),
    authority: textAt(fieldAt(record, "authority", path), `${path}.authority`),
    status: textAt(fieldAt(record, "status", path), `${path}.status`),
    provenance: parseProvenance(
      fieldAt(record, "provenance", path),
      `${path}.provenance`,
    ),
    createdAt: textAt(fieldAt(record, "createdAt", path), `${path}.createdAt`),
    updatedAt: textAt(fieldAt(record, "updatedAt", path), `${path}.updatedAt`),
    version: numberAt(fieldAt(record, "version", path), `${path}.version`),
  }
  const applicability = parseApplicability(record, path)
  if (applicability !== undefined) node.applicability = applicability
  const lastVerifiedAt = optionalTextAt(record, "lastVerifiedAt", path)
  if (lastVerifiedAt !== undefined) node.lastVerifiedAt = lastVerifiedAt
  return node
}

/** A node with everything the engine holds about it. */
export function parseEngineNodeDetail(value: unknown, path = "detail"): EngineNodeDetail {
  const record = recordAt(value, path)
  const detail: EngineNodeDetail = {
    node: parseEngineNode(fieldAt(record, "node", path), `${path}.node`),
    relationships: listField(record, "relationships", path, parseEngineRelationship),
    evidence: listField(record, "evidence", path, parseEngineEvidence),
    history: listField(record, "history", path, parseEngineVersion),
    utility: parseNodeUtility(fieldAt(record, "utility", path), `${path}.utility`),
  }
  if (record.page !== undefined && record.page !== null) {
    detail.page = parseEnginePage(record.page, `${path}.page`)
  }
  return detail
}

function parseNodeUtility(value: unknown, path: string): EngineNodeDetail["utility"] {
  const record = recordAt(value, path)
  return {
    retrievals: numberAt(fieldAt(record, "retrievals", path), `${path}.retrievals`),
    uses: numberAt(fieldAt(record, "uses", path), `${path}.uses`),
    citations: numberAt(fieldAt(record, "citations", path), `${path}.citations`),
    verifications: numberAt(fieldAt(record, "verifications", path), `${path}.verifications`),
    rejections: numberAt(fieldAt(record, "rejections", path), `${path}.rejections`),
  }
}

export function parseEnginePage(value: unknown, path = "page"): EnginePage {
  const record = recordAt(value, path)
  const page: EnginePage = {
    path: textAt(fieldAt(record, "path", path), `${path}.path`),
    contentHash: textAt(fieldAt(record, "contentHash", path), `${path}.contentHash`),
  }
  const absolutePath = optionalTextAt(record, "absolutePath", path)
  if (absolutePath !== undefined) page.absolutePath = absolutePath
  return page
}

export function parseEngineAnchor(value: unknown, path = "anchor"): EngineAnchor {
  const record = recordAt(value, path)
  const anchor: EngineAnchor = {
    file: textAt(fieldAt(record, "file", path), `${path}.file`),
  }
  const symbol = optionalTextAt(record, "symbol", path)
  if (symbol !== undefined) anchor.symbol = symbol
  const lineStart = optionalNumberAt(record, "lineStart", path)
  if (lineStart !== undefined) anchor.lineStart = lineStart
  const lineEnd = optionalNumberAt(record, "lineEnd", path)
  if (lineEnd !== undefined) anchor.lineEnd = lineEnd
  const regionHash = optionalTextAt(record, "regionHash", path)
  if (regionHash !== undefined) anchor.regionHash = regionHash
  const contentHash = optionalTextAt(record, "contentHash", path)
  if (contentHash !== undefined) anchor.contentHash = contentHash
  const commit = optionalTextAt(record, "commit", path)
  if (commit !== undefined) anchor.commit = commit
  return anchor
}

export function parseEngineEvidence(value: unknown, path = "evidence"): EngineEvidence {
  const record = recordAt(value, path)
  const evidence: EngineEvidence = {
    id: textAt(fieldAt(record, "id", path), `${path}.id`),
    node: textAt(fieldAt(record, "node", path), `${path}.node`),
    kind: textAt(fieldAt(record, "kind", path), `${path}.kind`),
    createdAt: textAt(fieldAt(record, "createdAt", path), `${path}.createdAt`),
  }
  if (record.anchor !== undefined && record.anchor !== null) {
    evidence.anchor = parseEngineAnchor(record.anchor, `${path}.anchor`)
  }
  const blobHash = optionalTextAt(record, "blobHash", path)
  if (blobHash !== undefined) evidence.blobHash = blobHash
  const excerpt = optionalTextAt(record, "excerpt", path)
  if (excerpt !== undefined) evidence.excerpt = excerpt
  const note = optionalTextAt(record, "note", path)
  if (note !== undefined) evidence.note = note
  return evidence
}

export function parseEngineVersion(value: unknown, path = "version"): EngineVersion {
  const record = recordAt(value, path)
  const version: EngineVersion = {
    node: textAt(fieldAt(record, "node", path), `${path}.node`),
    version: numberAt(fieldAt(record, "version", path), `${path}.version`),
    title: textAt(fieldAt(record, "title", path), `${path}.title`),
    summary: textAt(fieldAt(record, "summary", path), `${path}.summary`),
    content: textAt(fieldAt(record, "content", path), `${path}.content`),
    authority: textAt(fieldAt(record, "authority", path), `${path}.authority`),
    status: textAt(fieldAt(record, "status", path), `${path}.status`),
    recordedAt: textAt(fieldAt(record, "recordedAt", path), `${path}.recordedAt`),
  }
  const applicability = parseApplicability(record, path)
  if (applicability !== undefined) version.applicability = applicability
  const reason = optionalTextAt(record, "reason", path)
  if (reason !== undefined) version.reason = reason
  return version
}

export function parseEngineRelationship(value: unknown, path = "relationship"): EngineRelationship {
  const record = recordAt(value, path)
  const relationship: EngineRelationship = {
    id: textAt(fieldAt(record, "id", path), `${path}.id`),
    title: textAt(fieldAt(record, "title", path), `${path}.title`),
    type: textAt(fieldAt(record, "type", path), `${path}.type`),
    edgeType: textAt(fieldAt(record, "edgeType", path), `${path}.edgeType`),
    edgeId: textAt(fieldAt(record, "edgeId", path), `${path}.edgeId`),
    outbound: booleanAt(fieldAt(record, "outbound", path), `${path}.outbound`),
    established: booleanAt(fieldAt(record, "established", path), `${path}.established`),
  }
  // Additive in v1: absent means the engine stated neither, never that the
  // relationship is certain or unexplained.
  const note = optionalTextAt(record, "note", path)
  if (note !== undefined) relationship.note = note
  const confidence = optionalNumberAt(record, "confidence", path)
  if (confidence !== undefined) relationship.confidence = confidence
  return relationship
}

export function parseEngineProposal(value: unknown, path = "proposal"): EngineProposal {
  const record = recordAt(value, path)
  return {
    node: parseEngineNode(fieldAt(record, "node", path), `${path}.node`),
    relationships: listField(record, "relationships", path, parseEngineRelationship),
    evidence: listField(record, "evidence", path, parseEngineEvidence),
  }
}

function parseBriefNode(value: unknown, path: string): EngineBrief["nodes"][number] {
  const record = recordAt(value, path)
  const node: EngineBrief["nodes"][number] = {
    id: textAt(fieldAt(record, "id", path), `${path}.id`),
    title: textAt(fieldAt(record, "title", path), `${path}.title`),
    type: textAt(fieldAt(record, "type", path), `${path}.type`),
    summary: textAt(fieldAt(record, "summary", path), `${path}.summary`),
    authority: textAt(fieldAt(record, "authority", path), `${path}.authority`),
    status: textAt(fieldAt(record, "status", path), `${path}.status`),
    depth: numberAt(fieldAt(record, "depth", path), `${path}.depth`),
  }
  if (record.page !== undefined && record.page !== null) {
    node.page = parseEnginePage(record.page, `${path}.page`)
  }
  return node
}

export function parseEngineContext(value: unknown, path = "context"): EngineContext {
  const record = recordAt(value, path)
  const briefRecord = recordAt(fieldAt(record, "brief", path), `${path}.brief`)
  return {
    brief: {
      task: textAt(fieldAt(briefRecord, "task", `${path}.brief`), `${path}.brief.task`),
      nodes: listField(briefRecord, "nodes", `${path}.brief`, parseBriefNode),
      relationships: listField(
        briefRecord,
        "relationships",
        `${path}.brief`,
        (item, itemPath) => {
          const relationship = recordAt(item, itemPath)
          return {
            from: textAt(fieldAt(relationship, "from", itemPath), `${itemPath}.from`),
            type: textAt(fieldAt(relationship, "type", itemPath), `${itemPath}.type`),
            to: textAt(fieldAt(relationship, "to", itemPath), `${itemPath}.to`),
          }
        },
      ),
      anchors: textArrayField(briefRecord, "anchors", `${path}.brief`),
      warnings: textArrayField(briefRecord, "warnings", `${path}.brief`),
      truncated: booleanAt(
        fieldAt(briefRecord, "truncated", `${path}.brief`),
        `${path}.brief.truncated`,
      ),
    },
    prompt: textAt(fieldAt(record, "prompt", path), `${path}.prompt`),
  }
}

export function parseEngineGraphPath(value: unknown, path = "path"): EngineGraphPath {
  const record = recordAt(value, path)
  return {
    from: textAt(fieldAt(record, "from", path), `${path}.from`),
    to: textAt(fieldAt(record, "to", path), `${path}.to`),
    found: booleanAt(fieldAt(record, "found", path), `${path}.found`),
    steps: listField(record, "steps", path, (item, itemPath) => {
      const step = recordAt(item, itemPath)
      const parsed: EngineGraphPath["steps"][number] = {
        from: textAt(fieldAt(step, "from", itemPath), `${itemPath}.from`),
        edgeType: textAt(fieldAt(step, "edgeType", itemPath), `${itemPath}.edgeType`),
      }
      const note = optionalTextAt(step, "note", itemPath)
      if (note !== undefined) parsed.note = note
      const confidence = optionalNumberAt(step, "confidence", itemPath)
      if (confidence !== undefined) parsed.confidence = confidence
      return parsed
    }),
    nodes: listField(record, "nodes", path, parseEngineNode),
  }
}

export function parseLibraryLink(value: unknown, path = "library"): LibraryLink {
  const record = recordAt(value, path)
  return {
    workspacePath: textAt(fieldAt(record, "workspacePath", path), `${path}.workspacePath`),
    manifestPresent: booleanAt(fieldAt(record, "manifestPresent", path), `${path}.manifestPresent`),
    repositoryId: nullableFieldAt(record, "repositoryId", path),
    repositoryName: nullableFieldAt(record, "repositoryName", path),
    linked: booleanAt(fieldAt(record, "linked", path), `${path}.linked`),
    server: nullableFieldAt(record, "server", path),
    remoteId: nullableFieldAt(record, "remoteId", path),
    remoteName: nullableFieldAt(record, "remoteName", path),
    lastSynced: nullableFieldAt(record, "lastSynced", path),
    lastSyncAt: nullableFieldAt(record, "lastSyncAt", path),
    conflicts: textArrayField(record, "conflicts", path),
  }
}

/**
 * Validate one engine answer and narrow it to its method.
 *
 * This is the boundary: nothing reaches a store or a component without having
 * passed through here, so a drifted wire shape becomes one visible error, not
 * an `undefined` rendered as fact.
 */
export function parseEngineResponse(value: unknown): EngineResponse {
  const record = recordAt(value, "response")
  const method = textAt(fieldAt(record, "method", "response"), "response.method")
  switch (method) {
    case "status":
      return { method, status: parseEngineStatus(fieldAt(record, "status", "response")) }
    case "review":
      return { method, findings: listField(record, "findings", "response", parseEngineFinding) }
    case "sources":
      return { method, sources: listField(record, "sources", "response", parseEngineSource) }
    case "index":
      return { method, index: parseEngineIndexReport(fieldAt(record, "index", "response")) }
    case "proposals":
      return { method, proposals: listField(record, "proposals", "response", parseEngineProposal) }
    case "context":
      return { method, context: parseEngineContext(fieldAt(record, "context", "response")) }
    case "open":
      return { method, detail: parseEngineNodeDetail(fieldAt(record, "detail", "response")) }
    case "history":
      return { method, versions: listField(record, "versions", "response", parseEngineVersion) }
    case "path":
      return { method, path: parseEngineGraphPath(fieldAt(record, "path", "response")) }
    default:
      protocolError("response.method", "one of the methods this app asked for", method)
  }
}
