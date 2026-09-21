/**
 * Lint, from the frontend's side of the seam.
 *
 * What is asserted here is what this layer still owns: the prompt semantic
 * lint builds, the findings it parses back, and the mapping of a backend
 * lint report onto the list the panel renders.
 *
 * What is *not* asserted here is which pages are orphans, which links are
 * broken, or which page a suggestion should name. Those rules live in
 * `athenaeum-core`'s vault lint, where they are tested against a real vault
 * on disk — `vault::lint::tests`. A second copy of those expectations in
 * TypeScript would be the two-models problem this change exists to end.
 */
import { describe, it, expect, beforeEach, vi } from "vitest"
import type { LlmConfig } from "@/stores/wiki-store"
import type { IndexGraph, VaultLintReport } from "@/lib/knowledge-index"

vi.mock("./llm-client", () => ({
  streamChat: vi.fn(),
}))
vi.mock("@/lib/knowledge-index", () => ({
  lintVault: vi.fn(),
  loadKnowledgeGraph: vi.fn(),
}))

import { runSemanticLint, runStructuralLint } from "./lint"
import { streamChat } from "./llm-client"
import { lintVault, loadKnowledgeGraph } from "@/lib/knowledge-index"
import { useWikiStore } from "@/stores/wiki-store"
import { useActivityStore } from "@/stores/activity-store"

const mockStreamChat = vi.mocked(streamChat)
const mockLintVault = vi.mocked(lintVault)
const mockLoadGraph = vi.mocked(loadKnowledgeGraph)

function fakeLlmConfig(): LlmConfig {
  return {
    provider: "openai",
    apiKey: "k",
    model: "m",
    ollamaUrl: "",
    customEndpoint: "",
    maxContextSize: 128000,
  }
}

/** A page as the index projects it: a path, a title, a summary. */
function indexedPage(path: string, title: string, summary: string) {
  return {
    id: `vault_page_${path}`,
    title,
    kind: "concept",
    type: "concept",
    path,
    summary,
    authority: "unverified_observation",
    status: "valid",
    origin: "page" as const,
    linkCount: 0,
  }
}

function graphOf(...nodes: ReturnType<typeof indexedPage>[]): IndexGraph {
  return { nodes, edges: [] }
}

function lintReport(overrides: Partial<VaultLintReport> = {}): VaultLintReport {
  return {
    pages: 0,
    findings: [],
    scanComplete: true,
    brokenLinksTruncated: false,
    ...overrides,
  }
}

function respondWithTokens(text: string) {
  mockStreamChat.mockImplementation(async (_config, _messages, callbacks) => {
    if (text) callbacks.onToken(text)
    callbacks.onDone()
  })
}

beforeEach(() => {
  mockStreamChat.mockReset()
  mockLintVault.mockReset()
  mockLoadGraph.mockReset()
  mockLintVault.mockResolvedValue(lintReport())
  mockLoadGraph.mockResolvedValue({ nodes: [], edges: [] })
  useWikiStore.getState().setOutputLanguage("auto")
  useActivityStore.setState({ items: [] })
})

describe("runStructuralLint", () => {
  it("returns what the index found, unchanged", async () => {
    mockLintVault.mockResolvedValue(
      lintReport({
        pages: 2,
        findings: [
          {
            type: "broken-link",
            severity: "warning",
            page: "wiki/attention.md",
            node: "vault_page_a",
            detail: "Broken link: [[transfomer]] — target page not found.",
            brokenTarget: "transfomer",
            suggestedTarget: "wiki/transformer.md",
          },
          {
            type: "orphan",
            severity: "info",
            page: "wiki/transformer.md",
            node: "vault_page_t",
            detail: "No other pages link to this page.",
            suggestedSource: "wiki/attention.md",
          },
        ],
      }),
    )

    const results = await runStructuralLint("/project")

    expect(results).toEqual([
      {
        type: "broken-link",
        severity: "warning",
        page: "wiki/attention.md",
        detail: "Broken link: [[transfomer]] — target page not found.",
        brokenTarget: "transfomer",
        suggestedTarget: "wiki/transformer.md",
        suggestedSource: undefined,
      },
      {
        type: "orphan",
        severity: "info",
        page: "wiki/transformer.md",
        detail: "No other pages link to this page.",
        brokenTarget: undefined,
        suggestedTarget: undefined,
        suggestedSource: "wiki/attention.md",
      },
    ])
  })

  it("passes the user's ignore settings through to the index", async () => {
    await runStructuralLint("/project", {
      config: { ignoreOrphan: true, ignoreNoOutlinks: false, ignorePages: ["archive/old"] },
    })

    expect(mockLintVault).toHaveBeenCalledWith("/project", {
      ignoreOrphan: true,
      ignoreNoOutlinks: false,
      ignorePages: ["archive/old"],
    })
  })

  it("does not start a pass that has already been cancelled", async () => {
    const controller = new AbortController()
    controller.abort()

    await expect(
      runStructuralLint("/project", { signal: controller.signal }),
    ).rejects.toMatchObject({ name: "AbortError" })
    expect(mockLintVault).not.toHaveBeenCalled()
  })

  it("discards a result that arrives after cancellation", async () => {
    // The user's click has to win even when the backend was already busy:
    // otherwise a cancelled lint still repopulates the panel.
    const controller = new AbortController()
    mockLintVault.mockImplementation(async () => {
      controller.abort()
      return lintReport({ pages: 1 })
    })

    await expect(
      runStructuralLint("/project", { signal: controller.signal }),
    ).rejects.toMatchObject({ name: "AbortError" })
  })
})

describe("runSemanticLint — language directive", () => {
  it("uses explicit user setting", async () => {
    mockLoadGraph.mockResolvedValue(
      graphOf(
        indexedPage("wiki/a.md", "A", "Page A content here"),
        indexedPage("wiki/b.md", "B", "Page B content here"),
      ),
    )
    respondWithTokens("")

    useWikiStore.getState().setOutputLanguage("Korean")
    await runSemanticLint("/project", fakeLlmConfig())

    const prompt = mockStreamChat.mock.calls[0][1][0].content
    expect(prompt).toContain("MANDATORY OUTPUT LANGUAGE: Korean")
  })

  it("auto mode detects from the concatenated page summaries", async () => {
    const cjk = "这是一篇关于注意力机制和神经网络的长中文页面"
    mockLoadGraph.mockResolvedValue(
      graphOf(
        indexedPage("wiki/attention.md", "注意力", cjk),
        indexedPage("wiki/transformer.md", "变换器", cjk),
      ),
    )
    respondWithTokens("")

    useWikiStore.getState().setOutputLanguage("auto")
    await runSemanticLint("/project", fakeLlmConfig())

    const prompt = mockStreamChat.mock.calls[0][1][0].content
    expect(prompt).toContain("MANDATORY OUTPUT LANGUAGE: Chinese")
  })

  it("explicit setting wins over source language", async () => {
    mockLoadGraph.mockResolvedValue(
      graphOf(indexedPage("wiki/x.md", "X", "これは日本語の内容です")),
    )
    respondWithTokens("")

    useWikiStore.getState().setOutputLanguage("English")
    await runSemanticLint("/project", fakeLlmConfig())

    const prompt = mockStreamChat.mock.calls[0][1][0].content
    expect(prompt).toContain("MANDATORY OUTPUT LANGUAGE: English")
    expect(prompt).not.toContain("MANDATORY OUTPUT LANGUAGE: Japanese")
  })
})

describe("runSemanticLint — the pages it reasons about", () => {
  it("sends each page's path and the summary the index derived", async () => {
    mockLoadGraph.mockResolvedValue(
      graphOf(
        indexedPage("wiki/concepts/attention.md", "Attention", "Weighting tokens by relevance."),
      ),
    )
    respondWithTokens("")

    await runSemanticLint("/project", fakeLlmConfig())

    const prompt = mockStreamChat.mock.calls[0][1][0].content
    expect(prompt).toContain("### wiki/concepts/attention.md")
    expect(prompt).toContain("Weighting tokens by relevance.")
  })

  it("ignores knowledge that is not a page", async () => {
    // A source file and a node the agent proposed are both knowledge, and
    // neither is a wiki page a quality analyst can comment on.
    mockLoadGraph.mockResolvedValue({
      nodes: [
        indexedPage("wiki/a.md", "A", "A page."),
        { ...indexedPage("raw/sources/paper.pdf", "paper.pdf", "A source."), origin: "source" as const },
        { ...indexedPage("", "A decision", "Curated."), origin: "curated" as const, path: undefined },
      ],
      edges: [],
    })
    respondWithTokens("")

    await runSemanticLint("/project", fakeLlmConfig())

    const prompt = mockStreamChat.mock.calls[0][1][0].content
    expect(prompt).toContain("### wiki/a.md")
    expect(prompt).not.toContain("paper.pdf")
    expect(prompt).not.toContain("A decision")
  })
})

describe("runSemanticLint — missing-page false positives (#537)", () => {
  it("drops missing-page findings when the entity page already exists", async () => {
    mockLoadGraph.mockResolvedValue(
      graphOf(
        indexedPage("wiki/概念页.md", "概念页", "介绍维特根斯坦、阿德勒和尼采的概念页"),
        indexedPage("wiki/维特根斯坦.md", "维特根斯坦", "维特根斯坦是一位哲学家。"),
        indexedPage("wiki/阿德勒.md", "阿德勒", "阿德勒是一位心理学家。"),
      ),
    )
    // The LLM wrongly reports missing-page for entities that already have
    // pages, using free-form Chinese titles that match no fixed prefix.
    respondWithTokens(
      [
        "---LINT: missing-page | warning | 维特根斯坦---",
        "维特根斯坦在概念页中被引用但未建立独立实体页。",
        "PAGES: 概念页.md",
        "---END LINT---",
        "---LINT: missing-page | warning | 缺失页面：阿德勒---",
        "阿德勒被引用但缺少页面。",
        "PAGES: 概念页.md",
        "---END LINT---",
        "---LINT: missing-page | warning | 尼采---",
        "尼采被引用但没有页面。",
        "PAGES: 概念页.md",
        "---END LINT---",
      ].join("\n"),
    )

    const results = await runSemanticLint("/project", fakeLlmConfig())

    const missingPages = results.filter((r) => r.detail.includes("[missing-page]"))
    expect(missingPages).toHaveLength(1)
    expect(missingPages[0].page).toBe("尼采")
  })

  it("does not suppress an unrelated finding that merely contains an existing short title", async () => {
    mockLoadGraph.mockResolvedValue(
      graphOf(
        indexedPage("wiki/AI.md", "AI", "Artificial intelligence."),
        indexedPage("wiki/overview.md", "Overview", "References FAIR data governance."),
      ),
    )
    respondWithTokens(
      [
        "---LINT: missing-page | warning | FAIR data governance---",
        "The concept has no dedicated page.",
        "PAGES: overview.md",
        "---END LINT---",
      ].join("\n"),
    )

    const results = await runSemanticLint("/project", fakeLlmConfig())

    expect(results).toHaveLength(1)
    expect(results[0].page).toBe("FAIR data governance")
  })

  it("recognises a page by its file stem as well as its title", async () => {
    // A page titled "Attention Mechanism" at `attention-mechanism.md` is
    // named either way, and the model picks whichever it likes.
    mockLoadGraph.mockResolvedValue(
      graphOf(indexedPage("wiki/attention-mechanism.md", "Attention Mechanism", "Weights.")),
    )
    respondWithTokens(
      [
        "---LINT: missing-page | warning | attention-mechanism---",
        "No page for this concept.",
        "PAGES: wiki/attention-mechanism.md",
        "---END LINT---",
      ].join("\n"),
    )

    const results = await runSemanticLint("/project", fakeLlmConfig())

    expect(results).toEqual([])
  })
})

describe("runSemanticLint — activity & early returns", () => {
  it("logs a running activity item and marks done", async () => {
    mockLoadGraph.mockResolvedValue(graphOf(indexedPage("wiki/a.md", "A", "content")))
    respondWithTokens("")

    await runSemanticLint("/project", fakeLlmConfig())

    const items = useActivityStore.getState().items
    expect(items).toHaveLength(1)
    expect(items[0].type).toBe("lint")
    expect(["done", "error"]).toContain(items[0].status)
  })

  it("returns empty and marks done when the wiki has no pages", async () => {
    mockLoadGraph.mockResolvedValue({ nodes: [], edges: [] })

    const result = await runSemanticLint("/project", fakeLlmConfig())

    expect(result).toEqual([])
    expect(mockStreamChat).not.toHaveBeenCalled()
    expect(useActivityStore.getState().items[0].detail).toMatch(/no wiki pages/i)
  })

  it("marks error status when the index cannot be read", async () => {
    mockLoadGraph.mockRejectedValue(new Error("no project is open"))

    await runSemanticLint("/project", fakeLlmConfig())

    expect(useActivityStore.getState().items[0].status).toBe("error")
  })
})
