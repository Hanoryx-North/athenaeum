/**
 * The frontend's single door to the knowledge index.
 *
 * Before this existed the app held three descriptions of the same
 * knowledge: the graph view walked `wiki/` and re-extracted every
 * `[[wikilink]]`, the knowledge tree walked it again and re-parsed every
 * frontmatter block, and `athenaeum-core` held typed nodes and edges that
 * neither of them read. They disagreed in exactly the places that matter —
 * typed relationships, evidence, and anything that is knowledge without
 * being a file.
 *
 * There is now one model. Markdown on disk is still the source of truth;
 * this is the derived index over it, and every panel reads the same one.
 *
 * Indexing is a reconcile, not a rebuild, so calling `loadKnowledgeGraph`
 * after every change is cheap: a pass over an unchanged vault writes
 * nothing. The cache below is about avoiding redundant IPC round trips
 * within one data version, not about avoiding expensive work.
 */
import { invoke } from "@tauri-apps/api/core"
import { normalizePath } from "@/lib/path-utils"
import { api, type KnowledgeNode, type SearchHit } from "@/lib/knowledge-api"

/** Where a node came from, which decides what clicking it can do. */
export type IndexOrigin = "page" | "source" | "curated"

export interface IndexGraphNode {
  id: string
  title: string
  /**
   * The vault's own page type (`concept`, `query`, `synthesis`) for nodes
   * that came from a page, and the knowledge model's node type
   * (`decision`, `gotcha`) for everything else.
   */
  kind: string
  type: string
  /** Vault-relative path. Absent for knowledge that is not a file. */
  path?: string
  summary: string
  authority: string
  status: string
  origin: IndexOrigin
  /**
   * What a page declared about where it came from — `deep-research`,
   * `web-clip`. Absent for pages that declared nothing, and for knowledge
   * that is not a page.
   */
  declaredOrigin?: string
  linkCount: number
}

export interface IndexGraphEdge {
  from: string
  to: string
  type: string
  /** False for a relationship a model proposed but nobody has accepted. */
  deterministic: boolean
}

export interface IndexGraph {
  nodes: IndexGraphNode[]
  edges: IndexGraphEdge[]
}

export interface UnresolvedLink {
  from: string
  target: string
}

export interface VaultIndexReport {
  scanComplete: boolean
  pages: number
  sources: number
  linksResolved: number
  linksUnresolved: number
  nodesCreated: number
  nodesUpdated: number
  nodesRemoved: number
  edgesCreated: number
  edgesRemoved: number
  evidenceAttached: number
  evidenceUpdated: number
  evidenceRemoved: number
  unresolved: UnresolvedLink[]
  warnings: string[]
}

/** What a lint finding says is wrong. */
export type LintFindingType = "orphan" | "broken-link" | "no-outlinks"

export interface LintFinding {
  type: LintFindingType
  severity: "warning" | "info"
  /** Vault-relative path of the page, which is also the page a fix edits. */
  page: string
  /** The indexed node behind `page`. */
  node: string
  detail: string
  /** For a broken link, the target exactly as it was written. */
  brokenTarget?: string
  /** A page this one could link to. Vault-relative. */
  suggestedTarget?: string
  /** A page that could link to this one. Vault-relative. */
  suggestedSource?: string
}

export interface VaultLintReport {
  pages: number
  findings: LintFinding[]
  /** False when a scan budget truncated the pass behind these findings. */
  scanComplete: boolean
  /** True when there were more broken links than the indexer collects. */
  brokenLinksTruncated: boolean
}

/** Checks the user has switched off, and pages they have muted. */
export interface LintIgnores {
  ignoreOrphan?: boolean
  ignoreNoOutlinks?: boolean
  ignorePages?: string[]
}

/**
 * What the wiki links at, and what links at nothing.
 *
 * Orphans, broken links and dead ends are questions about edges, and the
 * index is where the edges are. Answering them here rather than by walking
 * `wiki/` again is what keeps the link-resolution rule single: the broken
 * links reported are the ones the indexer could not resolve, not a second
 * opinion about the same markdown.
 */
export async function lintVault(
  projectPath: string,
  options: LintIgnores = {},
): Promise<VaultLintReport> {
  await openKnowledgeBase(projectPath)
  return invoke<VaultLintReport>("lint_vault", {
    options: {
      ignoreOrphan: options.ignoreOrphan === true,
      ignoreNoOutlinks: options.ignoreNoOutlinks === true,
      ignorePages: options.ignorePages ?? [],
    },
  })
}

/**
 * How many projects hold a cached graph. Two, because switching back and
 * forth between a pair of projects is common and holding every project's
 * graph forever is not.
 */
const MAX_CACHED_PROJECTS = 2

const graphCache = new Map<string, { dataVersion: number; graph: IndexGraph }>()
const inFlight = new Map<string, Promise<IndexGraph>>()

/**
 * The project whose knowledge base is currently open in the Rust layer.
 *
 * The store is a single connection held by the backend, so opening a
 * second project replaces the first. Tracking it here keeps us from
 * re-opening the same one on every keystroke.
 */
let openProjectPath: string | null = null

/**
 * Open the knowledge base for a project, creating `.athenaeum/` if needed.
 *
 * Safe to call repeatedly. Call it when a project is opened so the first
 * panel to ask for knowledge does not pay for the handshake.
 */
export async function openKnowledgeBase(projectPath: string): Promise<void> {
  const pp = normalizePath(projectPath)
  if (openProjectPath === pp) return
  await api.openWorkspace(pp)
  openProjectPath = pp
}

/**
 * Reconcile the index against the vault and report what changed.
 *
 * Writes only differences, so this is cheap enough to call after a save.
 */
export async function indexVault(projectPath: string): Promise<VaultIndexReport> {
  await openKnowledgeBase(projectPath)
  return invoke<VaultIndexReport>("index_vault")
}

/**
 * The live knowledge graph for a project, indexed first so it reflects
 * whatever the vault says right now.
 *
 * `dataVersion` comes from the wiki store and changes whenever anything
 * writes to the vault. Passing it enables caching; omitting it forces a
 * fresh read.
 */
export async function loadKnowledgeGraph(
  projectPath: string,
  dataVersion?: number,
): Promise<IndexGraph> {
  const pp = normalizePath(projectPath)
  if (dataVersion !== undefined) {
    const cached = graphCache.get(pp)
    if (cached?.dataVersion === dataVersion) return cached.graph
  }

  const key = `${pp}::${dataVersion ?? "uncached"}`
  const pending = inFlight.get(key)
  if (pending) return pending

  const load = (async () => {
    await indexVault(pp)
    return invoke<IndexGraph>("knowledge_graph")
  })()
  inFlight.set(key, load)

  let graph: IndexGraph
  try {
    graph = await load
  } finally {
    inFlight.delete(key)
  }

  if (dataVersion !== undefined) {
    const cached = graphCache.get(pp)
    // A slower load started at an older data version must never overwrite
    // a newer graph that already landed.
    if (!cached || cached.dataVersion <= dataVersion) {
      if (!cached && graphCache.size >= MAX_CACHED_PROJECTS) {
        const oldest = graphCache.keys().next().value
        if (oldest) graphCache.delete(oldest)
      }
      graphCache.set(pp, { dataVersion, graph })
    }
  }
  return graph
}

/**
 * Full-text search over the index.
 *
 * This is the knowledge model's own ranking — a title match weighted above
 * a body match — over node summaries and content, which for a wiki page is
 * the page.
 */
export async function searchKnowledge(
  projectPath: string,
  query: string,
  limit = 20,
): Promise<SearchHit[]> {
  if (!query.trim()) return []
  await openKnowledgeBase(projectPath)
  return api.search(query, limit)
}

/** One node with everything the detail panels show. */
export async function knowledgeNode(
  projectPath: string,
  id: string,
): Promise<KnowledgeNode> {
  await openKnowledgeBase(projectPath)
  return (await api.nodeDetail(id)).node
}

/**
 * Forget cached graphs. Called when a project closes, so the next project
 * cannot be shown the previous one's knowledge for a frame.
 */
export function clearKnowledgeIndexCache(): void {
  graphCache.clear()
  inFlight.clear()
  openProjectPath = null
}

/** Absolute path for a node that is backed by a file, else null. */
export function nodeFilePath(
  projectPath: string,
  node: Pick<IndexGraphNode, "path">,
): string | null {
  if (!node.path) return null
  return `${normalizePath(projectPath).replace(/\/+$/, "")}/${node.path}`
}
