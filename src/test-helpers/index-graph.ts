/**
 * Standing in for the knowledge index in tests.
 *
 * Panels no longer walk `wiki/` — they ask `knowledge-index.ts`, which asks
 * Rust. A test that needs the app to see a wiki therefore needs a graph,
 * not a file tree, and a test that has a real directory on disk needs a way
 * to turn it into one.
 *
 * This is a stub for a mocked boundary, not a second indexer. It knows only
 * enough about a page to name it; what a page really becomes — its type,
 * its edges, its evidence, its summary — is decided by
 * `athenaeum-core`'s vault indexer and tested there, against a vault on
 * disk. Anything that depends on those rules belongs in a Rust test.
 */
import type { Dirent } from "node:fs"
import fs from "node:fs/promises"
import path from "node:path"
import type { IndexGraph, IndexGraphNode } from "@/lib/knowledge-index"

/** One indexed page, with everything the panels read off a node. */
export function indexedPage(
  vaultPath: string,
  overrides: Partial<IndexGraphNode> = {},
): IndexGraphNode {
  const stem = vaultPath.split("/").pop()?.replace(/\.md$/i, "") ?? vaultPath
  return {
    id: `vault_page_${vaultPath}`,
    title: stem,
    kind: "concept",
    type: "concept",
    path: vaultPath,
    summary: "",
    authority: "unverified_observation",
    status: "valid",
    origin: "page",
    linkCount: 0,
    ...overrides,
  }
}

export function graphOf(nodes: IndexGraphNode[]): IndexGraph {
  return { nodes, edges: [] }
}

/**
 * The graph the index would hold for a project whose wiki is on disk.
 *
 * `index.md` and `log.md` are left out, because the indexer leaves them
 * out: one is a table of contents the node table already is, the other a
 * journal the event journal already is.
 */
export async function graphFromWikiDir(projectPath: string): Promise<IndexGraph> {
  const root = path.join(projectPath, "wiki")
  const nodes: IndexGraphNode[] = []

  async function walk(dir: string): Promise<void> {
    let entries: Dirent[]
    try {
      entries = await fs.readdir(dir, { withFileTypes: true })
    } catch {
      return
    }
    for (const entry of entries) {
      const full = path.join(dir, entry.name)
      if (entry.isDirectory()) {
        await walk(full)
        continue
      }
      if (!entry.name.endsWith(".md")) continue
      if (entry.name === "index.md" || entry.name === "log.md") continue

      const content = await fs.readFile(full, "utf-8")
      const vaultPath = `wiki/${path
        .relative(root, full)
        .split(path.sep)
        .join("/")}`
      nodes.push(indexedPage(vaultPath, { title: titleOf(content, vaultPath) }))
    }
  }

  await walk(root)
  return graphOf(nodes)
}

/** Frontmatter title, else first heading, else the file stem. */
function titleOf(content: string, vaultPath: string): string {
  const frontmatter = content.match(/^---\s*\n([\s\S]*?)\n---/)
  const declared = frontmatter?.[1].match(/^title:\s*["']?(.+?)["']?\s*$/m)?.[1]?.trim()
  if (declared) return declared
  const heading = content.match(/^#\s+(.+)$/m)?.[1]?.trim()
  if (heading) return heading
  return vaultPath.split("/").pop()?.replace(/\.md$/i, "") ?? vaultPath
}
