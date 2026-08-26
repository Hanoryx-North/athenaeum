import { invoke } from "@tauri-apps/api/core"
import { normalizePath } from "@/lib/path-utils"
import { useWikiStore } from "@/stores/wiki-store"
import { loadKnowledgeGraph, nodeFilePath, searchKnowledge } from "@/lib/knowledge-index"

export interface ImageRef {
  url: string
  alt: string
}

export interface SearchResult {
  path: string
  title: string
  snippet: string
  titleMatch: boolean
  score: number
  vectorScore?: number
  images: ImageRef[]
}

interface BackendSearchResponse {
  // Reserved for result badges/debug UI. The backend already returns these
  // signals so API and WebView search share the same retrieval contract.
  mode: "keyword" | "vector" | "hybrid"
  results: SearchResult[]
  tokenHits: number
  vectorHits: number
  graphHits?: number
}

const STOP_WORDS = new Set([
  "的", "是", "了", "什么", "在", "有", "和", "与", "对", "从",
  "the", "is", "a", "an", "what", "how", "are", "was", "were",
  "do", "does", "did", "be", "been", "being", "have", "has", "had",
  "it", "its", "in", "on", "at", "to", "for", "of", "with", "by",
  "this", "that", "these", "those",
])

export function tokenizeQuery(query: string): string[] {
  const rawTokens = query
    .toLowerCase()
    .split(/[\s,，。！？、；：""''（）()\-_/\\·~～…]+/)
    .filter((t) => t.length > 1)
    .filter((t) => !STOP_WORDS.has(t))

  const tokens: string[] = []
  for (const token of rawTokens) {
    const hasCJK = /[\u4e00-\u9fff\u3400-\u4dbf]/.test(token)
    if (hasCJK && token.length > 2) {
      const chars = [...token]
      for (let i = 0; i < chars.length - 1; i++) tokens.push(chars[i] + chars[i + 1])
      for (const ch of chars) {
        if (!STOP_WORDS.has(ch)) tokens.push(ch)
      }
      tokens.push(token)
    } else {
      tokens.push(token)
    }
  }
  return [...new Set(tokens)]
}

export async function searchWiki(
  projectPath: string,
  query: string,
): Promise<SearchResult[]> {
  if (!query.trim()) return []
  const pp = normalizePath(projectPath)
  const embCfg = useWikiStore.getState().embeddingConfig

  // Two searches over one body of knowledge, not two bodies of knowledge.
  //
  // The index ranks wiki pages the way the knowledge model does — a title
  // match counts for more than a body match — and knows nothing about
  // images, raw source text or embeddings. The backend covers exactly
  // those. So both run, and the index's ordering wins where they overlap.
  //
  // The index half is allowed to fail quietly: a project whose knowledge
  // base cannot be opened should still be searchable, just less well.
  const [response, ranked] = await Promise.all([
    invoke<BackendSearchResponse>("search_project", {
      projectPath: pp,
      query,
      topK: 20,
      includeContent: false,
      queryEmbedding: null,
      embeddingConfig: embCfg,
    }),
    rankByIndex(pp, query).catch(() => []),
  ])

  const fileResults = response.results.map((result) => ({
    ...result,
    path: `${pp}/${normalizePath(result.path).replace(/^\/+/, "")}`,
  }))
  return mergeWithIndex(ranked, fileResults)
}

/**
 * Wiki pages the index matched, in its own order, as search results.
 *
 * Paths come from the cached graph rather than a second round trip: the
 * index keys knowledge by node id, and the path a page lives at is one of
 * the things the graph already carries.
 */
async function rankByIndex(projectPath: string, query: string): Promise<SearchResult[]> {
  const dataVersion = useWikiStore.getState().dataVersion
  const [hits, graph] = await Promise.all([
    searchKnowledge(projectPath, query),
    loadKnowledgeGraph(projectPath, dataVersion),
  ])
  const paths = new Map(graph.nodes.map((node) => [node.id, nodeFilePath(projectPath, node)]))
  const tokens = tokenizeQuery(query)

  const results: SearchResult[] = []
  for (const hit of hits) {
    const path = paths.get(hit.node.id)
    if (!path) continue // knowledge that is not a file has nothing to open
    const title = hit.node.title
    results.push({
      path,
      title,
      snippet: hit.node.summary,
      titleMatch: tokens.some((token) => title.toLowerCase().includes(token)),
      score: hit.score,
      images: [],
    })
  }
  return results
}

/**
 * One ranked list from the two searches.
 *
 * A page both found keeps the backend's record — it has the highlighted
 * snippet and the images — but takes the index's position. Pages only the
 * backend found follow, in its order.
 */
function mergeWithIndex(
  ranked: readonly SearchResult[],
  fileResults: readonly SearchResult[],
): SearchResult[] {
  if (ranked.length === 0) return [...fileResults]

  const byPath = new Map(fileResults.map((result) => [result.path, result]))
  const merged: SearchResult[] = []
  const taken = new Set<string>()

  for (const hit of ranked) {
    const richer = byPath.get(hit.path)
    merged.push(richer ? { ...richer, score: hit.score } : hit)
    taken.add(hit.path)
  }
  for (const result of fileResults) {
    if (!taken.has(result.path)) merged.push(result)
  }
  return merged
}
