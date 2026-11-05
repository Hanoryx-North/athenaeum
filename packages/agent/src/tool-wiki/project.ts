/**
 * Projections and rendering for the knowledge tools.
 *
 * The tools decide what reaches the model, so nothing here passes a library payload through
 * untouched: each projection names the fields the model needs and drops the rest. That keeps
 * additive protocol fields — which consumers must tolerate — from silently growing the
 * context a knowledge lookup costs.
 *
 * Every function is pure, so a rendered result replays identically from the session log.
 *
 * @module athenaeum-agent/tool-wiki/project
 */

import type {
  WikiBriefNode,
  WikiEvidence,
  WikiGraphPath,
  WikiNode,
  WikiNodeVersion,
  WikiPage,
  WikiReached,
  WikiRelatedNode,
  WikiSearchHit,
} from '../protocol/index.ts'
import type { WikiProbe } from '../wiki/index.ts'

/** A node as the model sees it in a list. */
export interface NodeSummaryView {
  id: string
  title: string
  type: string
  summary: string
  authority: string
  status: string
}

/** A search hit: a node summary plus its lexical score. */
export interface SearchHitView extends NodeSummaryView {
  score: number
}

/** A node reached by a walk, with the relationship that reached it and that relationship's own reason. */
export interface ReachedView extends NodeSummaryView {
  depth: number
  viaFrom?: string
  viaEdgeType?: string
  viaNote?: string
  viaConfidence?: number
}

/** One relationship of a node. `established` is false for a model proposal awaiting review. */
export interface RelationshipView {
  id: string
  title: string
  type: string
  edgeType: string
  outbound: boolean
  established: boolean
  note?: string
  confidence?: number
}

/** One step of the path between two nodes, with the relationship's own justification. */
export interface PathStepView {
  from: string
  edgeType: string
  note?: string
  confidence?: number
}

/** How one node connects to another. `found` false is a normal answer: no path within the budget. */
export interface PathView {
  from: string
  to: string
  found: boolean
  steps: PathStepView[]
  nodes: NodeSummaryView[]
}

/** One evidence record, flattened to the anchor fields worth reading. */
export interface EvidenceView {
  kind: string
  file?: string
  symbol?: string
  lines?: string
  excerpt?: string
  note?: string
}

/** The file behind a node that is a page of the vault, as a model needs it. */
export interface PageView {
  path: string
  absolutePath?: string
}

/** One archived revision. */
export interface VersionView {
  version: number
  title: string
  summary: string
  reason?: string
  recordedAt: string
}

/** One node a brief carries, at the depth the walk reached it. */
export interface BriefNodeView extends NodeSummaryView {
  depth: number
  page?: PageView
}

/** One relationship a brief carries, by node title. */
export interface BriefRelationView {
  from: string
  type: string
  to: string
}

/** One proposal awaiting review: the proposed node, its claimed relationships, and how much evidence backs it. */
export interface ProposalView extends NodeSummaryView {
  evidence: number
  relationships: RelationshipView[]
}

/** Project a stored node into its list form. */
export function nodeSummary(node: WikiNode): NodeSummaryView {
  return {
    id: node.id,
    title: node.title,
    type: node.type,
    summary: node.summary,
    authority: node.authority,
    status: node.status,
  }
}

/** Project one search hit. */
export function searchHit(hit: WikiSearchHit): SearchHitView {
  return { ...nodeSummary(hit.node), score: hit.score }
}

/** Project one node a walk reached, keeping the step that reached it and why that step holds. */
export function reached(step: WikiReached): ReachedView {
  // Seeds carry no `via`; the wire omits it rather than sending null, and `??` covers a library
  // that sends null anyway without letting it reach the projection as a crash.
  const via = step.via ?? undefined
  const note = via?.note ?? undefined
  const confidence = via?.confidence ?? undefined
  return {
    ...nodeSummary(step.node),
    depth: step.depth,
    ...via === undefined ? {} : { viaFrom: via.from, viaEdgeType: via.edgeType },
    ...note === undefined ? {} : { viaNote: note },
    ...confidence === undefined ? {} : { viaConfidence: confidence },
  }
}

/** Project one relationship, keeping its own reason and confidence. */
export function relationship(related: WikiRelatedNode): RelationshipView {
  // The wire promises omission, never null; normalizing here keeps a null out of the output
  // schema, where a null `note` would fail the registry's validation of the whole tool result.
  const note = related.note ?? undefined
  const confidence = related.confidence ?? undefined
  return {
    id: related.id,
    title: related.title,
    type: related.type,
    edgeType: related.edgeType,
    outbound: related.outbound,
    established: related.established,
    ...note === undefined ? {} : { note },
    ...confidence === undefined ? {} : { confidence },
  }
}

/** Project the path between two nodes. */
export function graphPath(path: WikiGraphPath): PathView {
  return {
    from: path.from,
    to: path.to,
    found: path.found,
    steps: path.steps.map((step) => {
      const note = step.note ?? undefined
      const confidence = step.confidence ?? undefined
      return {
        from: step.from,
        edgeType: step.edgeType,
        ...note === undefined ? {} : { note },
        ...confidence === undefined ? {} : { confidence },
      }
    }),
    nodes: path.nodes.map(nodeSummary),
  }
}

/** Format an anchor's line range, when it has one. */
function lineRange(start: number | undefined, end: number | undefined): string | undefined {
  if (start === undefined) return undefined
  return end === undefined || end === start ? String(start) : `${start}-${end}`
}

/** Project one evidence record, flattening its anchor. */
export function evidence(record: WikiEvidence): EvidenceView {
  const anchor = record.anchor ?? undefined
  const lines = anchor === undefined ? undefined : lineRange(anchor.lineStart, anchor.lineEnd)
  const excerpt = record.excerpt ?? undefined
  const note = record.note ?? undefined
  return {
    kind: record.kind,
    ...anchor?.file === undefined ? {} : { file: anchor.file },
    ...anchor?.symbol === undefined ? {} : { symbol: anchor.symbol },
    ...lines === undefined ? {} : { lines },
    ...excerpt === undefined ? {} : { excerpt },
    ...note === undefined ? {} : { note },
  }
}

/**
 * Project a page pointer. The content hash stays behind: a model has no use for a digest, and
 * carrying it would only cost context.
 */
export function pageView(page: WikiPage | undefined): PageView | undefined {
  const present = page ?? undefined
  if (present === undefined) return undefined
  return {
    path: present.path,
    ...present.absolutePath === undefined ? {} : { absolutePath: present.absolutePath },
  }
}

/** The path a file tool should be given: the absolute one when the library could supply it. */
export function renderPage(page: PageView): string {
  return page.absolutePath ?? page.path
}

/** Project one archived revision. */
export function version(revision: WikiNodeVersion): VersionView {
  return {
    version: revision.version,
    title: revision.title,
    summary: revision.summary,
    ...revision.reason === undefined ? {} : { reason: revision.reason },
    recordedAt: revision.recordedAt,
  }
}

/** Project one node a brief carries, keeping its depth and its page pointer. */
export function briefNode(node: WikiBriefNode): BriefNodeView {
  const page = pageView(node.page)
  return {
    id: node.id,
    title: node.title,
    type: node.type,
    summary: node.summary,
    authority: node.authority,
    status: node.status,
    depth: node.depth,
    ...page === undefined ? {} : { page },
  }
}

/** Project one relationship a brief carries. */
export function briefRelation(relation: { readonly from: string; readonly type: string; readonly to: string }): BriefRelationView {
  return { from: relation.from, type: relation.type, to: relation.to }
}

/** Project one proposal awaiting review. */
export function proposal(entry: {
  readonly node: WikiNode
  readonly relationships: readonly WikiRelatedNode[]
  readonly evidence: readonly WikiEvidence[]
}): ProposalView {
  return {
    ...nodeSummary(entry.node),
    evidence: entry.evidence.length,
    relationships: entry.relationships.map(relationship),
  }
}

/** Render the proposals awaiting review, or say plainly that there are none. */
export function renderProposals(proposals: readonly ProposalView[]): string {
  if (proposals.length === 0) return 'No proposals are waiting for review.'
  return proposals.map((entry) => {
    const relationships = entry.relationships.length === 0 ? '' : `\n${renderRelationships(entry.relationships)}`
    return `- ${entry.title} [${entry.id}] (${entry.type}, ${entry.authority}, ${entry.status}) — `
      + `${entry.evidence} evidence record${entry.evidence === 1 ? '' : 's'}${relationships}`
  }).join('\n')
}

/** One line for a node in a list: identity, kind, authority, and what it claims. */
function nodeLine(node: NodeSummaryView, suffix = ''): string {
  const summary = node.summary.length > 0 ? ` — ${node.summary}` : ''
  return `- ${node.title} [${node.id}] (${node.type}, ${node.authority}, ${node.status})${suffix}${summary}`
}

/**
 * A relationship's own justification: why it holds, and how sure whoever asserted it was.
 *
 * The reason is the useful part of a relationship — a decision and the gotcha it constrains
 * are connected *because* of something, and knowing what is what makes the edge worth
 * following rather than merely visible. A library that predates these fields sends neither,
 * and the edge renders as it always did.
 */
function connectionReason(note: string | undefined, confidence: number | undefined): string {
  const parts: string[] = []
  if (note !== undefined && note.length > 0) parts.push(note)
  if (confidence !== undefined) parts.push(`confidence ${confidence}`)
  return parts.length === 0 ? '' : ` (${parts.join('; ')})`
}

/** Render search hits, or say plainly that nothing matched. */
export function renderHits(hits: readonly SearchHitView[]): string {
  if (hits.length === 0) return 'No knowledge matched. Search finds seeds; relationships are what lead to the rest.'
  return hits.map(hit => nodeLine(hit)).join('\n')
}

/** Render the nodes a walk reached, in walk order, showing how each was reached and why that step holds. */
export function renderReached(nodes: readonly ReachedView[]): string {
  if (nodes.length === 0) return 'The walk reached nothing.'
  return nodes.map((node) => {
    const via = node.viaFrom === undefined ? '' : ` via ${node.viaFrom} -${node.viaEdgeType ?? ''}->`
    return nodeLine(node, ` depth ${node.depth}${via}${connectionReason(node.viaNote, node.viaConfidence)}`)
  }).join('\n')
}

/** Render relationships, marking proposals that have not been promoted and showing why each edge holds. */
export function renderRelationships(relationships: readonly RelationshipView[]): string {
  if (relationships.length === 0) return 'No relationships.'
  return relationships.map((edge) => {
    const direction = edge.outbound ? '→' : '←'
    const proposed = edge.established ? '' : ' (proposed, not traversed)'
    return `- ${direction} ${edge.edgeType} ${edge.title} [${edge.id}] (${edge.type})${proposed}`
      + connectionReason(edge.note, edge.confidence)
  }).join('\n')
}

/**
 * Render how one node connects to another.
 *
 * A path that does not exist is a normal answer, not a failure: the two nodes are simply not
 * connected within the budget the caller gave, and saying that is more useful than an error.
 *
 * @param path - the path the library resolved.
 * @returns the step-by-step text, with each step's own reason when the library sends one.
 */
export function renderPath(path: PathView): string {
  // `from`/`to` are resolved node ids. The walk carries the nodes, so a found path names them
  // by title; a path that was not found has no nodes to name and stays with the ids the caller used.
  const from = path.nodes[0]?.title ?? path.from
  const to = path.nodes[path.nodes.length - 1]?.title ?? path.to
  if (!path.found) return `No path from ${from} to ${to} within the budget.`
  const steps = path.steps.map((step, index) => {
    const stepFrom = path.nodes[index]?.title ?? step.from
    const stepTo = path.nodes[index + 1]?.title ?? '?'
    return `- ${index + 1}. ${stepFrom} -${step.edgeType}-> ${stepTo}${connectionReason(step.note, step.confidence)}`
  })
  return [
    `${from} connects to ${to} in ${path.steps.length} step${path.steps.length === 1 ? '' : 's'}:`,
    ...steps,
  ].join('\n')
}

/** Render evidence records with their source locations. */
export function renderEvidence(records: readonly EvidenceView[]): string {
  if (records.length === 0) return 'No evidence is attached.'
  return records.map((record) => {
    const symbol = record.symbol === undefined ? '' : `:${record.symbol}`
    const lines = record.lines === undefined ? '' : `:${record.lines}`
    const location = record.file === undefined ? '' : ` ${record.file}${symbol}${lines}`
    const excerpt = record.excerpt === undefined ? '' : `\n  > ${record.excerpt}`
    const note = record.note === undefined ? '' : `\n  ${record.note}`
    return `- ${record.kind}${location}${excerpt}${note}`
  }).join('\n')
}

/** Render archived revisions, in the order the library returns them. */
export function renderVersions(versions: readonly VersionView[]): string {
  if (versions.length === 0) return 'This node has no earlier revisions.'
  return versions.map((revision) => {
    const reason = revision.reason === undefined ? '' : ` — ${revision.reason}`
    return `- v${revision.version} (${revision.recordedAt}) ${revision.title}${reason}\n  ${revision.summary}`
  }).join('\n')
}

/**
 * Render one brief for the model.
 *
 * The library's own `prompt` is the text: it already carries the trust wrapper stating that
 * retrieved knowledge is reference material which does not override instructions, grant
 * permissions or authorize tools. Rendering the structure here instead would drop that
 * framing, so this function only decides what an empty brief says.
 *
 * @param nodes - the nodes the brief reached.
 * @param prompt - the library's wrapped prompt, injected verbatim when there is anything to say.
 * @returns the text the model reads.
 */
export function renderBrief(nodes: readonly BriefNodeView[], prompt: string): string {
  if (nodes.length === 0) {
    return 'Nothing is recorded about this yet. The repository itself is the source of truth; '
      + 'do not read this as "the project has no conventions", only as "nothing has been written down".'
  }
  return prompt
}

/**
 * What a knowledge-status caller reads: the probe state, the library's numbers when it
 * answered, and the failure that replaced them when it did not.
 *
 * This is the flat shape both consumers share — the model-facing `wiki_status` result and the
 * seam's {@link WikiProbe} — so the wording that explains a state is written once.
 */
export interface StatusView {
  state: string
  initialized?: boolean
  hasVault?: boolean
  vaultPages?: number
  nodes?: number
  edges?: number
  proposals?: number
  findings?: number
  indexedAt?: string
  indexStale?: boolean
  engineVersion?: string
  protocolVersion?: number
  code?: string
  message?: string
}

/**
 * Flatten a probe into the shared status view.
 *
 * @param probe - the seam's probe result.
 * @returns the state plus the library's numbers, or the failure that replaced them.
 */
export function statusView(probe: WikiProbe): StatusView {
  if (probe.state !== 'ready' && probe.state !== 'absent') {
    return { state: probe.state, code: probe.failure.code, message: probe.failure.message }
  }
  const status = probe.status
  if (status === undefined) return { state: probe.state }
  return {
    state: probe.state,
    initialized: status.initialized,
    hasVault: status.hasVault,
    vaultPages: status.vaultPages,
    nodes: status.nodes,
    edges: status.edges,
    proposals: status.proposals,
    findings: status.findings,
    ...status.indexedAt === null ? {} : { indexedAt: status.indexedAt },
    indexStale: status.indexStale,
    engineVersion: status.engineVersion,
    protocolVersion: status.protocolVersion,
  }
}

/** How fresh the index is, in the terms the library distinguishes: never indexed vs drifted. */
function indexLine(indexedAt: string | undefined, indexStale: boolean | undefined): string {
  if (indexedAt === undefined) return 'never indexed'
  return indexStale === true ? `indexed ${indexedAt}, and the index has drifted since` : `indexed ${indexedAt}`
}

/** `1 node` / `2 nodes`, so the sentence a model reads is not visibly machine-written. */
function count(value: number, noun: string): string {
  return `${value} ${noun}${value === 1 ? '' : 's'}`
}

/**
 * Render what the knowledge layer can do right now.
 *
 * `indexedAt` absent means no index run has happened — which is not the same as a stale
 * index, and the library reports `indexStale: false` for it. Saying "never indexed" there is
 * the honest reading, and it is the difference between "re-index" and "the knowledge base is
 * empty".
 *
 * @param view - the flattened status view.
 * @returns the model-facing status text.
 */
export function renderStatus(view: StatusView): string {
  switch (view.state) {
    case 'ready': {
      const proposals = view.proposals ?? 0
      const findings = view.findings ?? 0
      return [
        `Knowledge base: ${count(view.nodes ?? 0, 'node')}, ${count(view.edges ?? 0, 'edge')}, ${indexLine(view.indexedAt, view.indexStale)}.`,
        ...proposals > 0 ? [`${count(proposals, 'proposal')} ${proposals === 1 ? 'is' : 'are'} waiting for a person to review; ${proposals === 1 ? 'it is' : 'they are'} not knowledge yet.`] : [],
        ...findings > 0 ? [`${count(findings, 'review finding')} ${findings === 1 ? 'is' : 'are'} recorded against it.`] : [],
        `Engine ${view.engineVersion ?? 'unknown'}, protocol v${view.protocolVersion ?? 0}.`,
      ].join(' ')
    }
    case 'absent':
      return 'This project has no knowledge base. Nothing is recorded, and no wiki_* tool can answer here.'
    case 'incompatible':
      return `The installed Athenaeum library is not one this bundle can use (${view.code ?? 'unknown'}): ${view.message ?? ''} `
        + 'Knowledge is unavailable until the CLI matches; carry on reading the repository.'
    case 'unreachable':
      return `The knowledge base could not be reached (${view.code ?? 'unknown'}): ${view.message ?? ''} `
        + 'This is not a reason to stop: the repository is the source of truth.'
    default:
      return `No knowledge layer is available (${view.code ?? 'unknown'}): ${view.message ?? ''}`
  }
}

/**
 * Render the probe of a knowledge layer.
 *
 * @param probe - the seam's probe result.
 * @returns the model-facing status text.
 */
export function renderProbe(probe: WikiProbe): string {
  return renderStatus(statusView(probe))
}
