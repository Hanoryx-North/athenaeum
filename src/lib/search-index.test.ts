/**
 * searchWiki over one knowledge model.
 *
 * The index ranks wiki pages; the backend covers raw source text, images
 * and embeddings. These tests pin the contract between them: neither
 * search is allowed to hide what the other found, and the index's ordering
 * wins where both matched the same page.
 */
import { describe, it, expect, vi, beforeEach } from "vitest"
import type { ImageRef } from "./search"

const mockInvoke = vi.fn()
const mockSearchKnowledge = vi.fn()
const mockLoadKnowledgeGraph = vi.fn()

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

vi.mock("@/lib/knowledge-index", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/knowledge-index")>()
  return {
    ...actual,
    searchKnowledge: (...args: unknown[]) => mockSearchKnowledge(...args),
    loadKnowledgeGraph: (...args: unknown[]) => mockLoadKnowledgeGraph(...args),
  }
})

vi.mock("@/stores/wiki-store", () => ({
  useWikiStore: {
    getState: () => ({ embeddingConfig: null, dataVersion: 1 }),
  },
}))

const { searchWiki } = await import("./search")

/** A hit as the index reports it, plus the node's place in the graph. */
function indexed(id: string, title: string, path: string, score: number) {
  return {
    hit: { node: { id, title, summary: `${title} summary` }, score },
    node: { id, path },
  }
}

/** The backend reports project-relative paths; searchWiki absolutizes them. */
function backendHit(path: string, title: string, score: number) {
  return {
    path,
    title,
    snippet: `…${title}…`,
    titleMatch: false,
    score,
    images: [] as ImageRef[],
  }
}

function respond(backend: ReturnType<typeof backendHit>[]) {
  mockInvoke.mockImplementation(async (command: string) => {
    if (command === "search_project") {
      return { mode: "keyword", results: backend, tokenHits: backend.length, vectorHits: 0 }
    }
    return {}
  })
}

function indexReturns(entries: ReturnType<typeof indexed>[]) {
  mockSearchKnowledge.mockResolvedValue(entries.map((e) => e.hit))
  mockLoadKnowledgeGraph.mockResolvedValue({ nodes: entries.map((e) => e.node), edges: [] })
}

beforeEach(() => {
  mockInvoke.mockReset()
  mockSearchKnowledge.mockReset()
  mockLoadKnowledgeGraph.mockReset()
})

describe("searchWiki", () => {
  it("does not search at all for an empty query", async () => {
    expect(await searchWiki("/project", "  ")).toEqual([])
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it("orders shared results the way the index ranks them", async () => {
    respond([backendHit("wiki/b.md", "B", 9), backendHit("wiki/a.md", "A", 8)])
    indexReturns([
      indexed("n1", "A", "wiki/a.md", 5),
      indexed("n2", "B", "wiki/b.md", 3),
    ])

    const results = await searchWiki("/project", "reasoning")

    expect(results.map((r) => r.path)).toEqual([
      "/project/wiki/a.md",
      "/project/wiki/b.md",
    ])
  })

  it("keeps the backend's snippet and images for a page both found", async () => {
    respond([
      {
        ...backendHit("wiki/a.md", "A", 8),
        images: [{ url: "x.png", alt: "a diagram" }],
      },
    ])
    indexReturns([indexed("n1", "A", "wiki/a.md", 5)])

    const [result] = await searchWiki("/project", "reasoning")

    expect(result.snippet).toBe("…A…")
    expect(result.images).toHaveLength(1)
    expect(result.score).toBe(5)
  })

  it("keeps what only the backend found, such as a raw source or an image", async () => {
    respond([backendHit("raw/sources/paper.pdf", "paper.pdf", 4)])
    indexReturns([indexed("n1", "A", "wiki/a.md", 5)])

    const results = await searchWiki("/project", "reasoning")

    expect(results.map((r) => r.path)).toEqual([
      "/project/wiki/a.md",
      "/project/raw/sources/paper.pdf",
    ])
  })

  it("skips knowledge that has no file behind it to open", async () => {
    respond([])
    mockSearchKnowledge.mockResolvedValue([
      { node: { id: "curated", title: "Use SQLite", summary: "" }, score: 9 },
    ])
    mockLoadKnowledgeGraph.mockResolvedValue({
      nodes: [{ id: "curated", path: undefined }],
      edges: [],
    })

    expect(await searchWiki("/project", "sqlite")).toEqual([])
  })

  it("still searches files when the knowledge base cannot be opened", async () => {
    // A broken index makes search worse, not unavailable.
    respond([backendHit("wiki/a.md", "A", 8)])
    mockSearchKnowledge.mockRejectedValue(new Error("no project is open"))
    mockLoadKnowledgeGraph.mockRejectedValue(new Error("no project is open"))

    const results = await searchWiki("/project", "reasoning")

    expect(results.map((r) => r.path)).toEqual(["/project/wiki/a.md"])
  })

  it("marks a title match so the view can weight it", async () => {
    respond([])
    indexReturns([indexed("n1", "Chain of Thought", "wiki/cot.md", 5)])

    const [result] = await searchWiki("/project", "thought")

    expect(result.titleMatch).toBe(true)
    expect(result.snippet).toBe("Chain of Thought summary")
  })
})
