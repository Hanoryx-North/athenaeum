/**
 * Wiki lint: what is disconnected, and what does not hold together.
 *
 * Two halves with different natures. **Structural** lint is a question
 * about edges — what links to nothing, what nothing links to, what links at
 * a page that is not there — and it is answered by the knowledge index,
 * through `lintVault`. Nothing here re-reads `wiki/` to answer it: the
 * indexer already resolved every link, and a second implementation of that
 * rule in TypeScript was a second opinion nothing else was tested against.
 *
 * **Semantic** lint is a question about meaning — contradictions, stale
 * claims, concepts referenced everywhere and written up nowhere — and only
 * a model can answer it. It reads the index too, for the page list and the
 * one-line summaries it sends, so the pages it reasons about are the pages
 * the knowledge base holds.
 */
import { streamChat } from "@/lib/llm-client"
import type { LlmConfig } from "@/stores/wiki-store"
import { useActivityStore } from "@/stores/activity-store"
import { normalizePath } from "@/lib/path-utils"
import { buildLanguageDirective } from "@/lib/output-language"
import { normalizeReviewTitle } from "@/lib/review-utils"
import { lintVault, loadKnowledgeGraph, type IndexGraphNode } from "@/lib/knowledge-index"
import type { LintConfig } from "@/lib/lint-config"

export interface LintResult {
  type: "orphan" | "broken-link" | "no-outlinks" | "semantic"
  severity: "warning" | "info"
  /**
   * Vault-relative path of the page — `wiki/concepts/x.md` — for a
   * structural finding, and the model's own title for a semantic one.
   */
  page: string
  detail: string
  affectedPages?: string[]
  brokenTarget?: string
  suggestedTarget?: string
  suggestedSource?: string
}

// ── helpers ───────────────────────────────────────────────────────────────────

/**
 * Normalize a name for missing-page existence comparison. NFKC folds full-width
 * and compatibility forms so CJK / full-width variants compare equal.
 */
function normalizeForExistence(s: string): string {
  return normalizeReviewTitle(s).normalize("NFKC").trim().toLowerCase()
}

/**
 * Decide whether an LLM `missing-page` finding actually refers to a page that
 * already exists. The LLM does not reliably cross-reference the file list, so it
 * flags entities whose page is already present. Only exact normalized names are
 * accepted here. Substring matching is unsafe because short, valid page titles
 * can also be ordinary words inside an unrelated missing-page finding.
 */
function missingPageAlreadyExists(
  llmTitle: string,
  existingPageNames: Set<string>,
): boolean {
  const norm = normalizeForExistence(llmTitle)
  if (!norm) return false
  return existingPageNames.has(norm)
}

/** The file stem of a vault-relative path, which is what a link names. */
function pageStem(path: string): string {
  return path.split("/").pop()?.replace(/\.md$/i, "") ?? path
}

// ── Structural lint ───────────────────────────────────────────────────────────

export interface StructuralLintOptions {
  signal?: AbortSignal
  onProgress?: (completed: number, total: number) => void
  config?: LintConfig
}

/**
 * Ask the index what is disconnected.
 *
 * One call: the backend indexes the vault and projects the findings out of
 * the nodes and edges it just reconciled. The pass is a reconcile rather
 * than a rebuild, so running lint after every edit costs a diff.
 */
export async function runStructuralLint(
  projectPath: string,
  options: StructuralLintOptions = {},
): Promise<LintResult[]> {
  if (options.signal?.aborted) {
    throw new DOMException("Structural lint cancelled", "AbortError")
  }
  options.onProgress?.(0, 1)

  const report = await lintVault(normalizePath(projectPath), options.config)

  // The work happened in one backend call, so there was nothing to report
  // progress through. Cancellation is checked on the way out instead: the
  // user's click still has to discard the result it arrived too late for.
  if (options.signal?.aborted) {
    throw new DOMException("Structural lint cancelled", "AbortError")
  }
  options.onProgress?.(report.pages, report.pages)

  return report.findings.map((finding) => ({
    type: finding.type,
    severity: finding.severity,
    page: finding.page,
    detail: finding.detail,
    brokenTarget: finding.brokenTarget,
    suggestedTarget: finding.suggestedTarget,
    suggestedSource: finding.suggestedSource,
  }))
}

// ── Semantic lint ─────────────────────────────────────────────────────────────

const LINT_BLOCK_REGEX =
  /---LINT:\s*([^\n|]+?)\s*\|\s*([^\n|]+?)\s*\|\s*([^\n-]+?)\s*---\n([\s\S]*?)---END LINT---/g

export async function runSemanticLint(
  projectPath: string,
  llmConfig: LlmConfig,
  signal?: AbortSignal,
): Promise<LintResult[]> {
  const pp = normalizePath(projectPath)
  const activity = useActivityStore.getState()
  const activityId = activity.addItem({
    type: "lint",
    title: "Semantic wiki lint",
    status: "running",
    detail: "Reading wiki pages...",
    filesWritten: [],
  })

  let pages: IndexGraphNode[]
  try {
    const graph = await loadKnowledgeGraph(pp)
    pages = graph.nodes.filter((node) => node.origin === "page")
  } catch (error) {
    activity.updateItem(activityId, {
      status: "error",
      detail: `Failed to read the knowledge index: ${error}`,
    })
    return []
  }

  // One line per page: its path, and the summary the index derived from it.
  // Sending the first 500 characters of every file instead would cost a read
  // per page and spend the budget on frontmatter and headings — the summary
  // is the page's own account of itself, which is what the model is being
  // asked to compare.
  //
  // `existingPageNames` collects each page's title and file stem, and filters
  // out `missing-page` findings naming a page that already exists (#537).
  const summaries: string[] = []
  const existingPageNames = new Set<string>()
  for (const page of pages) {
    if (signal?.aborted) throw new DOMException("Semantic lint cancelled", "AbortError")
    if (page.title) existingPageNames.add(normalizeForExistence(page.title))
    const path = page.path ?? page.title
    const stem = pageStem(path)
    if (stem) existingPageNames.add(normalizeForExistence(stem))
    summaries.push(`### ${path}\n${page.summary}`)
  }

  if (summaries.length === 0) {
    activity.updateItem(activityId, { status: "done", detail: "No wiki pages to lint." })
    return []
  }

  activity.updateItem(activityId, { detail: "Running LLM semantic analysis..." })

  // For auto-mode language detection, sample the concatenated summaries
  // so non-English wikis get a matching language directive.
  const summarySample = summaries.join("\n").slice(0, 2000)

  const prompt = [
    "You are a wiki quality analyst. Review the following wiki page summaries and identify issues.",
    "",
    buildLanguageDirective(summarySample),
    "",
    "For each issue, output exactly this format:",
    "",
    "---LINT: type | severity | Short title---",
    "Description of the issue.",
    "PAGES: page1.md, page2.md",
    "---END LINT---",
    "",
    "Types:",
    "- contradiction: two or more pages make conflicting claims",
    "- stale: information that appears outdated or superseded",
    "- missing-page: an important concept is heavily referenced but has no dedicated page",
    "- suggestion: a question or source worth adding to the wiki",
    "For missing-page findings, Short title must be only the exact missing concept or entity name, without explanatory prefixes or suffixes.",
    "",
    "Severities:",
    "- warning: should be addressed",
    "- info: nice to have",
    "",
    "Only report genuine issues. Do not invent problems. Output ONLY the ---LINT--- blocks, no other text.",
    "",
    "## Wiki Pages",
    "",
    summaries.join("\n\n"),
  ].join("\n")

  let raw = ""
  let hadError = false

  await streamChat(
    llmConfig,
    [{ role: "user", content: prompt }],
    {
      onToken: (token) => { raw += token },
      onDone: () => {},
      onError: (err) => {
        hadError = true
        activity.updateItem(activityId, {
          status: "error",
          detail: `LLM error: ${err.message}`,
        })
      },
    },
    signal,
  )

  if (signal?.aborted) throw new DOMException("Semantic lint cancelled", "AbortError")
  if (hadError) return []

  const results: LintResult[] = []
  const matches = raw.matchAll(LINT_BLOCK_REGEX)

  for (const match of matches) {
    const rawType = match[1].trim().toLowerCase()
    const severity = match[2].trim().toLowerCase()
    const title = match[3].trim()
    const body = match[4].trim()

    // Drop `missing-page` findings whose page already exists — the LLM often
    // flags entities that already have a page, especially in non-English wikis
    // where its free-form titles don't match a fixed prefix (#537).
    if (rawType === "missing-page" && missingPageAlreadyExists(title, existingPageNames)) {
      continue
    }

    const pagesMatch = body.match(/^PAGES:\s*(.+)$/m)
    const affectedPages = pagesMatch
      ? pagesMatch[1].split(",").map((p) => p.trim())
      : undefined

    const detail = body.replace(/^PAGES:.*$/m, "").trim()

    results.push({
      type: "semantic",
      severity: (severity === "warning" ? "warning" : "info") as LintResult["severity"],
      page: title,
      detail: `[${rawType}] ${detail}`,
      affectedPages,
    })
  }

  activity.updateItem(activityId, {
    status: "done",
    detail: `Found ${results.length} semantic issue(s).`,
  })

  return results
}
