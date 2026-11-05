/**
 * wiki-graph.test.ts — the canvas projection over the knowledge index.
 *
 * What this file used to test — reading `title:` and `type:` from
 * frontmatter without mistaking a body line for YAML, resolving `[[links]]`
 * across case and spacing, preferring an exact filename to a case-folded
 * alias — now happens in Rust, in `athenaeum-core`'s vault indexer, and is
 * tested there against a real vault on disk. Keeping a second copy of
 * those expectations here would mean two descriptions of one behaviour,
 * which is the exact problem this change set exists to remove.
 *
 * What is left is genuinely this module's: turning a knowledge graph into
 * something a canvas can draw.
 */
import { describe, it, expect, vi, beforeEach } from "vitest"
import type { IndexGraph, IndexGraphNode } from "@/lib/knowledge-index"

const mockLoadKnowledgeGraph = vi.fn()

vi.mock("@/lib/knowledge-index", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/knowledge-index")>()
  return {
    ...actual,
    loadKnowledgeGraph: (...args: unknown[]) => mockLoadKnowledgeGraph(...args),
  }
})

const { buildWikiGraph } = await import("../wiki-graph")

function node(overrides: Partial<IndexGraphNode> & { id: string }): IndexGraphNode {
  return {
    title: overrides.id,
    kind: "concept",
    type: "concept",
    path: `wiki/concepts/${overrides.id}.md`,
    summary: "",
    authority: "unverified_observation",
    status: "valid",
    origin: "page",
    linkCount: 0,
    ...overrides,
  }
}

function graph(partial: Partial<IndexGraph>): IndexGraph {
  return { nodes: [], edges: [], ...partial }
}

beforeEach(() => {
  mockLoadKnowledgeGraph.mockReset()
})

describe("buildWikiGraph", () => {
  it("draws the index's nodes with their vault paths made absolute", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [
          node({ id: "a", title: "Chain of Thought", kind: "concept" }),
          node({
            id: "b",
            title: "wei.pdf",
            kind: "source",
            type: "source",
            path: "raw/sources/wei.pdf",
            origin: "source",
          }),
        ],
        edges: [{ from: "a", to: "b", type: "derived_from", deterministic: true }],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.nodes).toHaveLength(2)
    expect(result.nodes[0]).toMatchObject({
      id: "a",
      label: "Chain of Thought",
      type: "concept",
      path: "/project/wiki/concepts/a.md",
      slug: "a",
      linkCount: 1,
    })
    expect(result.nodes[1]).toMatchObject({
      path: "/project/raw/sources/wei.pdf",
      origin: "source",
    })
  })

  it("keeps the vault's own vocabulary rather than the knowledge model's", async () => {
    // A thesis page is an `observation` to the knowledge model, but the
    // canvas should still colour and label it as the thesis the user wrote.
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({ nodes: [node({ id: "t", kind: "thesis", type: "observation" })] }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.nodes[0].type).toBe("thesis")
  })

  it("keeps unsaved curated knowledge off the page canvas", async () => {
    // Unsaved proposals belong in Review; the page canvas shows saved vault files.
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [
          node({ id: "d", title: "Use SQLite", kind: "decision", origin: "curated", path: undefined }),
        ],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.nodes).toEqual([])
    expect(result.edges).toEqual([])
  })

  it("keeps query pages off the canvas without hiding them from the index", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [node({ id: "keep" }), node({ id: "drop", kind: "query" })],
        edges: [{ from: "keep", to: "drop", type: "references", deterministic: true }],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.nodes.map((n) => n.id)).toEqual(["keep"])
    // And no line is left dangling into the node that is not drawn.
    expect(result.edges).toEqual([])
    expect(result.nodes[0].linkCount).toBe(0)
  })

  it("collapses a reciprocal pair of links into one line", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [node({ id: "a" }), node({ id: "b" })],
        edges: [
          { from: "a", to: "b", type: "references", deterministic: true },
          { from: "b", to: "a", type: "references", deterministic: true },
          { from: "a", to: "b", type: "related_to", deterministic: true },
        ],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.edges).toHaveLength(1)
    expect(result.nodes.every((n) => n.linkCount === 1)).toBe(true)
  })

  it("draws a model's proposal, but more weakly than a fact", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [node({ id: "a" }), node({ id: "b" }), node({ id: "c" })],
        edges: [
          { from: "a", to: "b", type: "related_to", deterministic: false },
          { from: "b", to: "c", type: "references", deterministic: true },
        ],
      }),
    )

    const result = await buildWikiGraph("/project")

    const proposed = result.edges.find((e) => e.type === "related_to")
    const derived = result.edges.find((e) => e.type === "references")
    expect(proposed?.deterministic).toBe(false)
    expect(derived!.weight).toBeGreaterThan(proposed!.weight)
  })

  it("prefers the verified relationship when a pair has both", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [node({ id: "a" }), node({ id: "b" })],
        edges: [
          { from: "a", to: "b", type: "related_to", deterministic: false },
          { from: "a", to: "b", type: "references", deterministic: true },
        ],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.edges).toHaveLength(1)
    expect(result.edges[0]).toMatchObject({ type: "references", deterministic: true, weight: 1 })
  })

  it("ignores an edge whose other end is not on the canvas", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(
      graph({
        nodes: [node({ id: "a" })],
        edges: [
          { from: "a", to: "ghost", type: "references", deterministic: true },
          { from: "a", to: "a", type: "related_to", deterministic: true },
        ],
      }),
    )

    const result = await buildWikiGraph("/project")

    expect(result.edges).toEqual([])
  })

  it("returns an empty graph for a project with no indexed knowledge", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(graph({}))

    expect(await buildWikiGraph("/empty")).toEqual({
      nodes: [],
      edges: [],
      communities: [],
    })
  })

  it("passes the data version through so the index layer can cache on it", async () => {
    mockLoadKnowledgeGraph.mockResolvedValue(graph({ nodes: [node({ id: "a" })] }))

    await buildWikiGraph("/project", 42)

    expect(mockLoadKnowledgeGraph).toHaveBeenCalledWith("/project", 42)
  })
})
