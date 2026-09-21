import { describe, it, expect, vi, beforeEach } from "vitest"

const mockInvoke = vi.fn()

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

const {
  clearKnowledgeIndexCache,
  indexVault,
  lintVault,
  loadKnowledgeGraph,
  nodeFilePath,
  openKnowledgeBase,
  searchKnowledge,
} = await import("./knowledge-index")

const EMPTY_GRAPH = { nodes: [], edges: [] }

/** Answer each command with something shaped like its real result. */
function respond(overrides: Record<string, unknown> = {}) {
  mockInvoke.mockImplementation(async (command: string) => {
    if (command in overrides) return overrides[command]
    if (command === "knowledge_graph") return EMPTY_GRAPH
    return {}
  })
}

function commands(): string[] {
  return mockInvoke.mock.calls.map((call) => call[0] as string)
}

beforeEach(() => {
  mockInvoke.mockReset()
  clearKnowledgeIndexCache()
})

describe("openKnowledgeBase", () => {
  it("opens the workspace once per project rather than per call", async () => {
    respond()

    await openKnowledgeBase("/project")
    await openKnowledgeBase("/project")

    expect(commands().filter((c) => c === "open_workspace")).toHaveLength(1)
  })

  it("reopens when the project changes", async () => {
    respond()

    await openKnowledgeBase("/one")
    await openKnowledgeBase("/two")

    expect(mockInvoke).toHaveBeenNthCalledWith(1, "open_workspace", { path: "/one" })
    expect(mockInvoke).toHaveBeenNthCalledWith(2, "open_workspace", { path: "/two" })
  })

  it("does not remember a project whose workspace failed to open", async () => {
    // Otherwise a transient failure would leave every later call believing
    // the store is open and reporting "no project is open" forever.
    mockInvoke.mockRejectedValueOnce("disk is full")
    await expect(openKnowledgeBase("/project")).rejects.toBe("disk is full")

    respond()
    await openKnowledgeBase("/project")
    expect(commands()).toContain("open_workspace")
  })
})

describe("loadKnowledgeGraph", () => {
  it("reconciles the index before reading it", async () => {
    // Reading a stale index is worse than paying for the pass: the pass
    // writes nothing when nothing changed.
    respond()

    await loadKnowledgeGraph("/project")

    expect(commands()).toEqual(["open_workspace", "index_vault", "knowledge_graph"])
  })

  it("reuses a graph within one data version", async () => {
    respond()

    const first = await loadKnowledgeGraph("/project", 7)
    const second = await loadKnowledgeGraph("/project", 7)

    expect(second).toBe(first)
    expect(commands().filter((c) => c === "knowledge_graph")).toHaveLength(1)
  })

  it("re-reads when the vault has changed", async () => {
    respond()

    await loadKnowledgeGraph("/project", 7)
    await loadKnowledgeGraph("/project", 8)

    expect(commands().filter((c) => c === "knowledge_graph")).toHaveLength(2)
  })

  it("never caches when no data version is given", async () => {
    respond()

    await loadKnowledgeGraph("/project")
    await loadKnowledgeGraph("/project")

    expect(commands().filter((c) => c === "knowledge_graph")).toHaveLength(2)
  })

  it("shares one round trip between concurrent callers", async () => {
    // The graph view, the knowledge tree and search can all ask at once
    // when a project opens.
    respond()

    const [a, b] = await Promise.all([
      loadKnowledgeGraph("/project", 1),
      loadKnowledgeGraph("/project", 1),
    ])

    expect(a).toBe(b)
    expect(commands().filter((c) => c === "knowledge_graph")).toHaveLength(1)
  })

  it("does not let a slow older read overwrite a newer graph", async () => {
    const older = { nodes: [{ id: "old" }], edges: [] }
    const newer = { nodes: [{ id: "new" }], edges: [] }
    let releaseOlder: (value: unknown) => void = () => {}
    const olderGraph = new Promise((resolve) => {
      releaseOlder = resolve
    })
    let seen = 0
    mockInvoke.mockImplementation(async (command: string) => {
      if (command !== "knowledge_graph") return {}
      seen += 1
      return seen === 1 ? olderGraph : newer
    })

    const slow = loadKnowledgeGraph("/project", 1)
    await loadKnowledgeGraph("/project", 2)
    releaseOlder(older)
    await slow

    // The cached graph is still the newer one.
    const cached = await loadKnowledgeGraph("/project", 2)
    expect(cached).toBe(newer)
  })

  it("stops caching a project once the cache is cleared", async () => {
    respond()

    await loadKnowledgeGraph("/project", 3)
    clearKnowledgeIndexCache()
    await loadKnowledgeGraph("/project", 3)

    expect(commands().filter((c) => c === "knowledge_graph")).toHaveLength(2)
  })
})

describe("indexVault", () => {
  it("reports what the pass changed", async () => {
    const report = { pages: 12, nodesCreated: 3, linksUnresolved: 1 }
    respond({ index_vault: report })

    expect(await indexVault("/project")).toBe(report)
  })
})

describe("lintVault", () => {
  it("opens the project before asking it for findings", async () => {
    // The backend lints the vault it has open; asking before opening one
    // answers "no project is open" rather than "nothing is wrong".
    respond({ lint_vault: { pages: 0, findings: [], scanComplete: true, brokenLinksTruncated: false } })

    await lintVault("/project")

    expect(commands()).toEqual(["open_workspace", "lint_vault"])
  })

  it("states every check explicitly, so an omitted option is off rather than absent", async () => {
    respond({ lint_vault: { pages: 0, findings: [], scanComplete: true, brokenLinksTruncated: false } })

    await lintVault("/project", { ignoreOrphan: true })

    expect(mockInvoke).toHaveBeenLastCalledWith("lint_vault", {
      options: { ignoreOrphan: true, ignoreNoOutlinks: false, ignorePages: [] },
    })
  })
})

describe("searchKnowledge", () => {
  it("does not go to the backend for an empty query", async () => {
    respond()

    expect(await searchKnowledge("/project", "   ")).toEqual([])
    expect(mockInvoke).not.toHaveBeenCalled()
  })

  it("searches the index with an explicit limit", async () => {
    respond({ search: [] })

    await searchKnowledge("/project", "reasoning", 5)

    expect(mockInvoke).toHaveBeenCalledWith("search", { query: "reasoning", limit: 5 })
  })
})

describe("nodeFilePath", () => {
  it("makes a vault-relative path absolute", () => {
    expect(nodeFilePath("/project/", { path: "wiki/concepts/a.md" })).toBe(
      "/project/wiki/concepts/a.md",
    )
  })

  it("has nothing to point at for knowledge that is not a file", () => {
    expect(nodeFilePath("/project", { path: undefined })).toBeNull()
  })
})
