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
| Graph view (sigma + graphology + Louvain) | `src/components/graph/`, `src/lib/wiki-graph*.ts` | Now reads the derived index; the worker, the layout and the filters are unchanged. |
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
| Chat panel (`src/components/chat/`) | Keep the UI. It now calls the installed Harness SDK runtime through a small Tauri-managed Node driver, and the selected Athenaeum project is the runtime workspace. |
| Chat store and event rendering | The existing local chat presentation receives Harness session and tool updates, then stores the final response. The Harness remains the owner of turns and sessions. |
| Settings (`src/components/settings/`, 23 files) | Drop provider/model/routing panes — `deepseek-harness` owns providers now. Keep vault, ingest, appearance, language, API, embedding settings. |
| Knowledge model glue | llm_wiki has no typed knowledge graph. Athenaeum's `athenaeum-core` indexes the vault: pages become nodes, `[[wikilinks]]` become deterministic edges, frontmatter carries type and authority. **Done** — see Status below. |
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
| `api_server.rs`, `clip_server.rs` | 3,988 | **Deferred, not refused.** The local HTTP API and clipper receiver; their only consumers are the MCP server and the extension, both out of scope, and the chat endpoints are agent surface that would have to be cut anyway. Porting them means re-taking `cors.rs`, `server_bind.rs` and `commands/page_embedding.rs`, removed here as orphans. | — |

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

`LICENSE` (GPL-3.0), [`provenance.md`](provenance.md) and the About panel in Settings carry
the repository-level record: upstream, author, licence.

`src/README.md` states that the whole frontend tree is ported, which is more honest than a
header on 300 largely verbatim files. Individual headers go on files that were
**substantially rewritten or synthesized** rather than copied — `src-tauri/src/lib.rs`,
`search_backends.rs`, `panic_guard.rs`, `vite.config.ts`, `lib/changelog.ts`:

```
// Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
// Modified for Athenaeum.
```

**Branding is not code.** llm_wiki's `logo.jpg` was not kept; the sidebar uses Athenaeum's
own generated icon. Upstream's name, logo and release history do not ship here.

## Status

Steps 1–7 and step 9 of the order above are done:

- Licence, provenance, manifests — done.
- Frontend foundation, layout, UI primitives, vault layer, editor, graph, sources, search,
  lint, review, ingest pipeline — ported; 1,814 frontend tests pass.
- Rust backend — document parsing, file sync, history, project management, vector search,
  search backends; 378 workspace tests.
- **Vault indexing** (step 5) — `crates/athenaeum-core/src/vault/` turns pages into typed
  nodes, `[[wikilinks]]` into deterministic edges, `raw/sources/` entries into source
  nodes and `sources:` frontmatter into evidence. Reconciles rather than rebuilds; the
  test that deleting `knowledge.db` costs only time is
  `vault::tests::a_deleted_database_costs_only_time`.
- **Graph, search and knowledge panels over the index** (step 6) — one knowledge model
  rather than one per panel. `src/lib/knowledge-index.ts` is the frontend's only door to
  it.
- **Lint, review and deep research over the index** (step 9) — structural lint is now a
  projection in `crates/athenaeum-core/src/vault/lint.rs` rather than a second
  link-resolver in TypeScript; semantic lint sends the index's pages and summaries;
  the review sweep asks the index what the wiki contains instead of re-parsing every
  frontmatter block; and deep research offers the model the indexed pages that bear on
  its topic instead of the raw text of `index.md`.
- `crates/athenaeum-providers` — retired and removed.

## Desktop chat

The app launches `@deepseek-ai/dsh-sdk-client` from the installed Athenaeum Agent
profile, using the current project directory and a stable session id for each chat.
The driver only translates a small newline-delimited control protocol; Harness owns
agent execution. The panel reports Harness status, session activity and tool results,
and Stop closes the runtime because the SDK has no per-turn cancellation method.
The agent bundle already exposes wiki read and curation tools. Curation records
proposals in the local engine; accepting in Review is what writes Markdown.

The remaining safety gap belongs to the Harness permission boundary: its ordinary
workspace file tools can still write `wiki/` directly when the profile grants that
access. The current SDK has no profile-level path deny that this app can apply. The
desktop does not convert chat text into accepted knowledge, but it cannot yet guarantee
that every file edit to a wiki page passes through proposal review.

Still open, in order:

1. **Protect accepted wiki pages from direct tool writes.** Add this through a documented
   Harness permission extension when one is available; do not fork Harness or implement
   a second permission system in Athenaeum.
2. **Restyle toward SiYuan's density** (step 10).
3. Optionally, the local HTTP API, once something in scope needs it.

### What the vault index does and does not claim

Worth stating, because both halves are decisions rather than omissions:

- **A page's identity is its path**, not its title. Renaming a page's `title:` keeps its
  node and its history; moving the file is a different page. Guessing that a moved file
  is "the same page" merges two histories irreversibly when the guess is wrong.
- **`index.md` and `log.md` are not nodes.** One is a table of contents that the node
  table *is*, the other an activity journal that the event journal *is*. Indexing them
  would hang two link-to-everything hubs off the graph and bury every real relationship
  under them. Both stay in the vault, readable and editable.
- **A deleted page's node is deleted, not marked stale.** The index is a function of the
  vault; a page that is gone has no node. This is what makes a reconciled index and a
  rebuilt one identical, which is the property the whole design rests on.
- **Arbitrary frontmatter is not modelled.** `tags:`, `authors:`, `year:` stay in the
  markdown. The index keeps what markdown cannot answer quickly — typed edges, evidence,
  authority, traversal, search — and a page's own YAML is not that.
