# Porting llm_wiki into Athenaeum

Athenaeum's UI and wiki engine are ported from
[llm_wiki](https://github.com/nashsu/llm_wiki) (GPL-3.0, © Yong Su) rather than written
from scratch. This document decides, subsystem by subsystem, what is copied, what is
adapted, and what is deliberately refused.

**Read this before copying anything.** The refusals matter more than the copies.

## Why llm_wiki and not neural-notes

The look being aimed for is neural-notes — which is [SiYuan](https://github.com/siyuan-note/siyuan)
v3.8.1. It cannot be ported:

| | neural-notes (SiYuan) | llm_wiki | Athenaeum |
| --- | --- | --- | --- |
| Frontend | Vanilla TS + webpack | **React 19 + Tailwind 4** | **React + Tailwind** |
| Shell | Electron | **Tauri 2** | **Tauri 2** |
| Backend | Go kernel over HTTP | **Rust in-process** | **Rust in-process** |
| Editor | `protyle`, own `.sy` block format | Milkdown over markdown | markdown |
| Licence | AGPL-3.0 | GPL-3.0 | GPL-3.0 (was MIT) |

SiYuan's interface is welded to its Go kernel's HTTP API and its own block format. Taking
it means replacing Athenaeum's entire stack, and inheriting AGPL-3.0 on top. llm_wiki is
already the stack Athenaeum uses, and is already the auto-wiki-builder this product is.

**So: port llm_wiki's code, and take SiYuan only as a visual reference** — spacing,
density, chrome, the calm of it. Look at it, do not copy from it. No SiYuan code, CSS or
assets enter this repository.

## The licence consequence

llm_wiki is GPL-3.0. Code copied from it carries that licence, so **Athenaeum is GPL-3.0**
(it was MIT). Recorded in [`provenance.md`](provenance.md).

This stops at Athenaeum's edge. `athenaeum_agent` and `deepseek-harness` stay MIT: separate
repositories, separate processes, connected only by the `athenaeum bridge` protocol, which
is a data format rather than linkage. **Do not copy Athenaeum code into either of them.**

## Where knowledge lives after the port

The two repositories model knowledge differently, and the merge resolves it one way:

**Markdown files on disk are the source of truth.** llm_wiki's vault layout wins — `raw/sources/`,
`wiki/*.md` with YAML frontmatter and `[[wikilink]]`s, `index.md`, `log.md`, `purpose.md`,
`schema.md` — because it opens in Obsidian, and the agent can edit it with ordinary file
tools.

**Athenaeum's SQLite store becomes a derived index over that vault.** It keeps what
markdown cannot answer quickly: typed edges, evidence, authority, provenance, history,
full-text search, traversal, briefs, staleness.

The test of a correct port: **deleting `knowledge.db` loses nothing but time.** If it loses
knowledge, something was written to the wrong layer.

## Subsystem decisions

Sizes are llm_wiki's, to show where the work actually is.

### PORT — copy, rename, keep the design

| Subsystem | Where | Notes |
| --- | --- | --- |
| Three-column layout, icon sidebar, resizable panels | `src/components/layout/` (16 files) | The shell. Restyle toward SiYuan's density; keep the structure. |
| Knowledge tree, file tree, activity panel | `src/components/layout/` | |
| Wiki editor and reader (Milkdown), frontmatter and page-links panels | `src/components/editor/` (7 files) | |
| Graph view (sigma + graphology + Louvain) | `src/components/graph/`, `src/lib/wiki-graph*.ts` | Reads the derived index once wired; keep the worker. |
| Sources, search, lint, review panels | `src/components/{sources,search,lint,review}/` | |
| Project management, welcome screen, create/open | `src/components/project/` | |
| UI primitives (shadcn/base-ui) | `src/components/ui/` (8 files) | |
| Ingest pipeline — two-step chain-of-thought | `src/lib/` sweep/enrich/dedup/chunk | Athenaeum's auto wiki builder. Its direct model calls are legitimate: this is wiki building, not an agent loop. |
| Vault I/O, wikilinks, page types, schema, filenames | `src/lib/wiki-*.ts`, `wikilink-transform.ts` | |
| Source lifecycle, preprocessing, watching, dedup | `src/lib/source-*.ts`, `dedup*.ts` | |
| Document parsing (PDF, Office, EPUB, images, media) | `src-tauri/src/commands/` | Rust, drops straight in. |
| Vector search, embeddings, page embedding | `src/lib/embedding.ts`, `src-tauri/src/commands/vectorstore.rs` | Optional path; keep it optional. |
| Deep research, web search | `src/lib/deep-research.ts`, `web-search.ts` | |
| Stores (zustand) | `src/stores/` | Except the chat store — see ADAPT. |
| i18n | `src/i18n/` | |

### ADAPT — port, but rewire

| Subsystem | Change |
| --- | --- |
| Chat panel (`src/components/chat/`) | Keep the UI. **Replace its backend.** It talks to llm_wiki's in-process Rust agent today; it must talk to `athenaeum_agent` over the harness instead. This is the single most important rewire in the port. |
| Chat store, `chat-agent-types.ts`, `agent-file-activity.ts` | Retarget at the harness event stream. The rendering survives; the transport does not. |
| Settings (`src/components/settings/`, 23 files) | Drop provider/model/routing panes — `deepseek-harness` owns providers now. Keep vault, ingest, appearance, language, API, embedding settings. |
| Knowledge model glue | llm_wiki has no typed knowledge graph. Athenaeum's `athenaeum-core` indexes the vault: pages become nodes, `[[wikilinks]]` become deterministic edges, frontmatter carries type and authority. |
| Branding | `llm_wiki` / `LLM Wiki` → `athenaeum` / `Athenaeum` throughout. Bundle identifiers, window titles, config paths, store keys. |

### DO NOT PORT — owned elsewhere

**This is the section that prevents the overlap this project already hit once.**

| Subsystem | Size | Why not | Owned by |
| --- | --- | --- | --- |
| **`src-tauri/src/agent/`** | **13,512 lines** | A complete second coding-agent runtime: loop, provider, tools, sessions, permissions, cancellation, skills, router. Porting it means maintaining two agent runtimes. | `deepseek-harness` |
| `src-tauri/src/commands/claude_cli.rs`, `codex_cli.rs`, `cli_resolver.rs` | ~1k | Shelling out to external agent CLIs. The harness *is* the agent. | `deepseek-harness` |
| `src/lib/claude-cli-transport.ts`, `codex-cli-transport.ts` | | Same, frontend side. | `deepseek-harness` |
| Agent skills (`src-tauri/src/agent/skills.rs`) | 699 | The harness has a skill capability. | `deepseek-harness` |
| Agent permissions, workspace, cancel | | Harness capabilities. | `deepseek-harness` |
| Model provider config, routing, streaming | 1,460 (`agent/provider.rs`) | Providers are the harness's. Ingest calls a model through a thin client, not a provider framework. | `deepseek-harness` |
| MCP server (`mcp-server/`) | | Out of scope for now. The harness exposes tools its own way. | — |
| Chrome extension (`extension/`) | | Later, if wanted. | — |

If a task asks you to port anything in that table, stop and say which repository owns it.

### REPLACE — Athenaeum already has better

| llm_wiki has | Athenaeum keeps |
| --- | --- |
| Markdown-only knowledge, no typed relationships | `athenaeum-core`: typed nodes and edges, evidence, authority, provenance, history |
| Graph relevance by 4 heuristic signals | Real traversal, bidirectional navigation, shortest paths over the index |
| No source anchors | Source anchors with symbol/region fingerprint relocation |
| No knowledge briefs | `athenaeum brief` and the trust wrapper |
| No agent seam | `athenaeum bridge` protocol v1 |
| No deterministic repo indexing | Files, symbols, imports, Git history, no model involved |

### RETIRE — from Athenaeum

| Subsystem | Why |
| --- | --- |
| `crates/athenaeum-providers/` (2,327 lines) | Bootstrap model client from before the runtime split. `deepseek-harness` owns providers. Remove once nothing calls it. |
| `src/` current React shell (845 lines) | Superseded by the ported UI. |

## Order of work

Each step should leave the repository building.

1. **Licence and provenance** — GPL-3.0 `LICENSE`, manifest fields, `provenance.md`.
2. **Frontend foundation** — Vite, React 19, Tailwind 4, the dependency set; the existing
   845-line shell comes out.
3. **Layout and UI primitives** — shell, icon sidebar, panels, trees. First visible milestone.
4. **Vault layer** — read/write `raw/`, `wiki/`, `index.md`, `log.md`; editor and reader.
5. **Index the vault** — `athenaeum-core` builds nodes and edges from pages and wikilinks;
   prove `knowledge.db` is rebuildable and losing it costs nothing.
6. **Graph, search, sources panels** — over the index.
7. **Ingest pipeline** — the auto wiki builder end to end.
8. **Chat rewired to `athenaeum_agent`** — the seam closes here. Needs the harness built and
   the bundle mounted.
9. **Lint, review, deep research.**
10. **Restyle toward SiYuan's density**, and retire `athenaeum-providers`.

Steps 1–7 need no agent. Step 8 is where the three repositories first run together.

## Attribution

Every ported file keeps its origin recorded. Per-file header on substantially copied files:

```
// Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
// Modified for Athenaeum.
```

`provenance.md` carries the repository-level record: upstream, version, commit, licence.
