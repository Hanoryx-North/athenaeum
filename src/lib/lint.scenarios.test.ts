/**
 * Scenario-driven semantic lint tests.
 *
 * Each scenario is a small wiki plus the answer a model gave about it. The
 * wiki reaches lint the way it reaches it in the app — through the
 * knowledge index — so these scenarios describe indexed pages rather than
 * files on disk. Structural scenarios moved to `vault::lint::tests` in
 * `athenaeum-core` along with the rules they assert.
 */
import { describe, it, expect, beforeEach, vi } from "vitest"
import { lintScenarios } from "@/test-helpers/scenarios/lint-scenarios"
import type { LintScenario } from "@/test-helpers/scenarios/types"
import type { IndexGraph } from "@/lib/knowledge-index"

let currentLlmResponse = ""
vi.mock("./llm-client", () => ({
  streamChat: vi.fn(async (_cfg, _msgs, cb) => {
    if (currentLlmResponse) cb.onToken(currentLlmResponse)
    cb.onDone()
  }),
}))
vi.mock("@/lib/knowledge-index", () => ({
  lintVault: vi.fn(),
  loadKnowledgeGraph: vi.fn(),
}))

import { runSemanticLint, type LintResult } from "./lint"
import { loadKnowledgeGraph } from "@/lib/knowledge-index"
import { useWikiStore } from "@/stores/wiki-store"
import { useActivityStore } from "@/stores/activity-store"

const mockLoadGraph = vi.mocked(loadKnowledgeGraph)

/**
 * The scenario's wiki, as the index would hold it.
 *
 * A stub, not a second indexer: the title comes off the frontmatter line
 * the scenario wrote and the summary is the first line of prose. What the
 * indexer really derives from a page is tested in Rust; what matters here
 * is that lint is handed pages rather than paths.
 */
function indexOf(scenario: LintScenario): IndexGraph {
  const nodes = Object.entries(scenario.initialWiki)
    .filter(([path]) => !/\/(index|log)\.md$/.test(path))
    .map(([path, content]) => ({
      id: `vault_page_${path}`,
      title: content.match(/^title:\s*(.+)$/m)?.[1].trim() ?? path,
      kind: "concept",
      type: "concept",
      path,
      summary:
        content
          .replace(/^---[\s\S]*?---\n/, "")
          .split("\n")
          .find((line) => line.trim() && !line.startsWith("#")) ?? "",
      authority: "unverified_observation",
      status: "valid",
      origin: "page" as const,
      linkCount: 0,
    }))
  return { nodes, edges: [] }
}

beforeEach(() => {
  currentLlmResponse = ""
  mockLoadGraph.mockReset()
  useActivityStore.setState({ items: [] })
  useWikiStore.getState().setLlmConfig({
    provider: "openai",
    apiKey: "test-key",
    model: "gpt-4",
    ollamaUrl: "",
    customEndpoint: "",
    maxContextSize: 128000,
  })
})

function assertSemantic(scenario: LintScenario, actual: LintResult[]): void {
  const expected = scenario.expected.semantic
  try {
    expect(actual.length).toBe(expected.length)
    for (const item of expected) {
      const match = actual.find(
        (finding) =>
          finding.type === item.type &&
          finding.severity === (item.severity as "warning" | "info") &&
          (item.titleContains === undefined ||
            finding.detail.includes(item.titleContains) ||
            finding.page.includes(item.titleContains)),
      )
      expect(match, `no semantic finding matching ${JSON.stringify(item)}`).toBeTruthy()
    }
  } catch (err) {
    // eslint-disable-next-line no-console
    console.error(
      `\n[lint: ${scenario.name}] semantic mismatch. Actual findings:\n` +
        JSON.stringify(actual, null, 2),
    )
    throw err
  }
}

describe("semantic lint scenarios (fixture-driven)", () => {
  it.each(lintScenarios.map((scenario) => [scenario.name, scenario] as const))(
    "%s",
    async (_name, scenario) => {
      mockLoadGraph.mockResolvedValue(indexOf(scenario))
      currentLlmResponse = scenario.llmResponse

      const findings = await runSemanticLint(
        "/project",
        useWikiStore.getState().llmConfig,
      )

      assertSemantic(scenario, findings)
    },
  )
})
