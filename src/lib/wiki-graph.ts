/**
 * The graph the view draws, built from the knowledge index.
 *
 * This used to walk `wiki/`, read every markdown file and re-extract every
 * `[[wikilink]]` on the way to the screen — a second knowledge model,
 * living alongside `athenaeum-core`'s and differing from it in the places
 * that matter. Now there is one model: the Rust indexer turns the vault's
 * markdown into typed nodes and edges, and this reads the result.
 *
 * What stayed here is presentation: community detection, edge weighting
 * and the decision about which node types belong on the canvas. Those
 * change whenever the visual design changes, and none of them belong in a
 * library that must stay free of the UI.
 */
import { normalizePath } from "@/lib/path-utils"
import {
  loadKnowledgeGraph,
  nodeFilePath,
  type IndexGraphEdge,
  type IndexGraphNode,
  type IndexOrigin,
} from "@/lib/knowledge-index"
import { detectCommunities } from "./wiki-graph-analysis"

export interface GraphNode {
  /** Stable knowledge-node id. Unique even when two pages share a slug. */
  id: string
  label: string
  type: string
  /** Absolute path, or `""` for knowledge that is not a file. */
  path: string
  linkCount: number // inbound + outbound
  community: number // community id from Louvain detection
  /** File stem, for the structural-page heuristics that key off names. */
  slug?: string
  summary?: string
  authority?: string
  origin?: IndexOrigin
}

export interface GraphEdge {
  source: string
  target: string
  weight: number // how strongly the two are related
  /** The typed relationship, e.g. `references`, `derived_from`. */
  type?: string
  /** False when a model proposed the relationship and nobody accepted it. */
  deterministic?: boolean
}

export interface CommunityInfo {
  id: number
  nodeCount: number
  cohesion: number // intra-community edge density
  topNodes: string[] // top nodes by linkCount (labels)
}

export interface WikiGraphResult {
  nodes: GraphNode[]
  edges: GraphEdge[]
  communities: CommunityInfo[]
}

const COMMUNITY_WORKER_THRESHOLD = 3_000

/**
 * Page types kept off the canvas.
 *
 * Query pages are research results and saved chat answers — intermediate
 * artifacts, not knowledge structure. The entities and concepts auto-ingest
 * extracts from them are what belong in the graph. They stay in the index,
 * so search and the knowledge tree still find them.
 */
const HIDDEN_KINDS = new Set(["query"])

/**
 * Weight for a relationship nobody has verified.
 *
 * Deterministic edges were read out of the markdown, so they are as true
 * as the vault is. A model's proposal is drawn, because reviewing
 * proposals is something a person does in the graph, but it pulls the
 * layout together less firmly than a fact does.
 */
const PROPOSED_EDGE_WEIGHT = 0.5

async function analyzeCommunities(
  nodes: { id: string; label: string; linkCount: number }[],
  edges: GraphEdge[],
): Promise<{ assignments: Map<string, number>; communities: CommunityInfo[] }> {
  if (nodes.length < COMMUNITY_WORKER_THRESHOLD || typeof Worker === "undefined") {
    return detectCommunities(nodes, edges)
  }

  return new Promise((resolve, reject) => {
    const worker = new Worker(
      new URL("./wiki-graph-analysis.worker.ts", import.meta.url),
      { type: "module" },
    )
    worker.onmessage = (event: MessageEvent<{
      assignments: [string, number][]
      communities: CommunityInfo[]
    }>) => {
      worker.terminate()
      resolve({
        assignments: new Map(event.data.assignments),
        communities: event.data.communities,
      })
    }
    worker.onerror = (event) => {
      worker.terminate()
      reject(new Error(event.message || "Knowledge graph analysis worker failed"))
    }
    worker.postMessage({ nodes, edges })
  })
}

export async function buildWikiGraph(
  projectPath: string,
  dataVersion?: number,
): Promise<WikiGraphResult> {
  const pp = normalizePath(projectPath)
  const index = await loadKnowledgeGraph(pp, dataVersion)

  const visible = new Map<string, IndexGraphNode>()
  for (const node of index.nodes) {
    if (HIDDEN_KINDS.has(node.kind)) continue
    // Reviewable memory belongs on the graph too. Keep repository extraction
    // artifacts out of this canvas, while exposing the claims users can review.
    if (node.origin === "curated" && !["decision", "gotcha", "procedure", "fact", "rule", "observation"].includes(node.kind)) continue
    visible.set(node.id, node)
  }
  if (visible.size === 0) {
    return { nodes: [], edges: [], communities: [] }
  }

  const edges = collapseToUndirected(index.edges, visible)

  // Recount links against what is actually drawn: the index counts every
  // relationship, and hiding a node or collapsing a reciprocal pair of
  // edges changes how connected the remaining nodes look.
  const linkCounts = new Map<string, number>()
  for (const id of visible.keys()) linkCounts.set(id, 0)
  for (const edge of edges) {
    linkCounts.set(edge.source, (linkCounts.get(edge.source) ?? 0) + 1)
    linkCounts.set(edge.target, (linkCounts.get(edge.target) ?? 0) + 1)
  }

  const prelimNodes = Array.from(visible.values()).map((node) => ({
    id: node.id,
    label: node.title,
    linkCount: linkCounts.get(node.id) ?? 0,
  }))
  const { assignments, communities } = await analyzeCommunities(prelimNodes, edges).catch(
    () => detectCommunities(prelimNodes, edges),
  )

  const nodes: GraphNode[] = Array.from(visible.values()).map((node) => ({
    id: node.id,
    label: node.title,
    type: node.kind,
    path: nodeFilePath(pp, node) ?? "",
    slug: slugOf(node.path),
    summary: node.summary,
    authority: node.authority,
    origin: node.origin,
    linkCount: linkCounts.get(node.id) ?? 0,
    community: assignments.get(node.id) ?? 0,
  }))

  return { nodes, edges, communities }
}

/**
 * One line per pair of nodes.
 *
 * The index stores direction and type, and a page can both reference
 * another and declare it related — three true statements that are still
 * one line on a canvas. The strongest weight wins so a verified
 * relationship is not diluted by a proposal alongside it.
 */
function collapseToUndirected(
  edges: readonly IndexGraphEdge[],
  visible: ReadonlyMap<string, IndexGraphNode>,
): GraphEdge[] {
  const byPair = new Map<string, GraphEdge>()
  for (const edge of edges) {
    if (edge.from === edge.to) continue
    if (!visible.has(edge.from) || !visible.has(edge.to)) continue

    const key = [edge.from, edge.to].sort().join("\u0000")
    const weight = edge.deterministic ? 1 : PROPOSED_EDGE_WEIGHT
    const existing = byPair.get(key)
    if (!existing) {
      byPair.set(key, {
        source: edge.from,
        target: edge.to,
        weight,
        type: edge.type,
        deterministic: edge.deterministic,
      })
      continue
    }
    if (weight > existing.weight) {
      existing.weight = weight
      existing.type = edge.type
      existing.deterministic = edge.deterministic
    }
  }
  return Array.from(byPair.values())
}

/** File stem of a vault-relative path, or `""` when there is no file. */
function slugOf(path: string | undefined): string {
  if (!path) return ""
  const name = path.split("/").pop() ?? path
  return name.replace(/\.md$/i, "")
}
