# AGENTS.md — Athenaeum

**Read this before writing any code in this repository.** It defines what Athenaeum is
allowed to contain. If a task asks you to build something on the *must not* list below,
stop and say which repository owns it instead.

> **Ownership was changed by the product owner on 2026-10-09.** `athenaeum` is the one
> canonical local engine and the main desktop and CLI product. Earlier text in this
> repository that describes `athenaeum` as "the visible layer" only, or that sends local
> logic to a separate `athenaeum_core` repository, is superseded by this file.

## What Athenaeum is

Athenaeum is **the app**: a complete local knowledge application a person opens, with the
agent experience of a coding-agent app, over a wiki that an AI agent writes and a person
reviews. It is also **the local logic layer**: the one engine that holds the knowledge, in
Markdown on disk, and answers every client through one versioned contract.

- **Local first.** Everything works offline. The Markdown vault is the source of truth.
  Graphs, search indexes, evidence indexes and caches are built and used locally and can be
  rebuilt from the vault at any time.
- **The cloud is a place to publish, not a place to think.** The hosted Library is a
  GitHub-like store for a person's *accepted wiki text*. It stores Markdown and the minimal
  metadata for identity and revision history, nothing else.
- **The agent is not in this repository.** Agent turns, tools and permissions run in
  `deepseek-harness`, extended by `athenaeum_agent`. Athenaeum is their client.

## Repository map — nine repositories, five kinds of thing

| Repository | Kind | What it is | Stack | Licence |
| --- | --- | --- | --- | --- |
| **`athenaeum`** — this one | **Application** (+ CLI + engine crate) | The desktop app, the vault, the one local engine (`athenaeum-core` crate), the `athenaeum` CLI, the local bridge, the sync client | Tauri 2 + React 19 + Rust | GPL-3.0 |
| `athenaeum_protocol` | Library | Versioned wire contracts and shared fixtures: bridge v1, sync v1, Library API v1 | TypeScript + Rust | MIT |
| `athenaeum_api` | Service | The hosted text backend: accounts, tokens, repositories, revisions, push, operator routes. No graph, no model | TypeScript (Hono) | MIT |
| `athenaeum_library` | Application (static web) | Public/private repository browsing UI for the hosted Library | React | MIT |
| `athenaeum_admin` | Application (static web) | Operator console for the hosted service | React | MIT |
| `athenaeum_agent` | Adapter | The `wiki` seam, `wiki_*` tools and knowledge brief as a harness bundle | TypeScript (DSH bundle) | MIT |
| `athenaeum_integrations` | Adapter | The MCP server and typed client for the bridge | TypeScript | MIT |
| `athenaeum_validation` | Test infrastructure | Cross-repository product tests, preserved suites, the runner | Python + TypeScript | — |
| `athenaeum_core` | **Retired** | A deprecated shim that forwards the old `athenaeum-core` command to `athenaeum` | Rust | private |
| `deepseek-ai/deepseek-harness` | External dependency | The agent runtime: loop, tools, permissions, sessions, providers. **Never forked** | TypeScript | MIT |

```text
┌──────────────────────────────────────────────────────────────────┐
│ athenaeum        the window · the vault · the ONE local engine     │
│                                                                    │
│  UI        sidebar · trees · editor · graph · review · agent chat  │
│  Vault     raw/sources/ · wiki/*.md · index.md · log.md            │
│            (Markdown on disk — the source of truth)                │
│  Engine    crates/athenaeum-core: nodes · edges · evidence ·       │
│            authority · search · briefs · SQLite index (derived)    │
│  Bridge    `athenaeum bridge` / in-process: protocol v1 JSON       │
│  Sync      explicit push/pull of accepted wiki text                │
└──────┬──────────────────────────────┬────────────────────────────┘
       │ bridge v1 (stdio JSON)       │ Library API v1 (HTTPS, text)
┌──────┴───────────────┐       ┌──────┴──────────────────────────┐
│ athenaeum_agent      │       │ athenaeum_api  ← athenaeum_library│
│ athenaeum_integrations│      │ (text storage)   athenaeum_admin │
│ (MCP / plugins)      │       └─────────────────────────────────┘
└──────┬───────────────┘
       │ mounted as a bundle
┌──────┴───────────────────────────────────────────────────────────┐
│ deepseek-harness   agent loop · tools · permissions · sessions    │
└──────────────────────────────────────────────────────────────────┘
```

## Athenaeum owns

- The desktop shell and every pixel of the app's UI, including the agent chat client.
- The **vault**: `raw/sources/`, `wiki/*.md`, `index.md`, `log.md`, `purpose.md`,
  `schema.md`, and the repository identity `athenaeum.json`. Markdown on disk,
  `[[wikilink]]`-connected and Obsidian-compatible.
- The **one local engine** (`crates/athenaeum-core`): nodes, typed edges, evidence,
  provenance, authority, history, full-text search, graph navigation, knowledge briefs, the
  proposal lifecycle (propose → review → accept into Markdown), and the derived SQLite
  index. It is rebuildable from the vault with `athenaeum index-vault`.
- The **auto wiki builder**: ingest, analysis, page generation, lint, review, deep research.
- The **installer** (`install/`) and `athenaeum setup` / `athenaeum doctor`: one install that puts the CLI
  and the agent integration on a machine and checks the wiring. They *orchestrate*; the agents' own
  configuration files are written by `athenaeum_integrations`, which `setup` runs.
- The `athenaeum` CLI, the **bridge** (`athenaeum bridge`, and the same code in-process for
  the desktop) and the **sync client** (`athenaeum library …`, `athenaeum sync …`).
- Repository indexing: files, symbols, local imports, Git history. Deterministic, no model.
- Migration of data from the retired second engine (`athenaeum import-legacy-core`).

## Athenaeum MUST NOT contain

| Do not build here | Owned by |
| --- | --- |
| Agent loop, turn handling, conversation state | `deepseek-harness` |
| Model providers, streaming, retries, token accounting for agent turns | `deepseek-harness` |
| Coding tools — read, write, edit, search, shell, git | `deepseek-harness` |
| Tool registry, tool schemas, tool permissions | `deepseek-harness` |
| Agent sessions, cancellation, context compaction | `deepseek-harness` |
| Subagents, workflows, skills, hooks | `deepseek-harness` |
| `wiki_*` model-facing tools, the knowledge brief that opens a task, *when* the agent asks | `athenaeum_agent` |
| MCP servers, plugin configuration for other agents | `athenaeum_integrations` |
| Hosted accounts, tokens, repositories, revisions, operator routes | `athenaeum_api` |
| The Library and operator web UIs | `athenaeum_library`, `athenaeum_admin` |
| Wire shapes shared with other repositories | `athenaeum_protocol` |
| **A second knowledge engine, graph or database** | nobody — there is one |

Consequences worth stating outright:

- **Athenaeum never runs a model loop.** It may call a model directly for *ingest*
  (source → wiki pages), because that is wiki building, not coding. It must never grow a
  tool-calling agent loop. Chat in the UI is a **client** of `athenaeum_agent`; the turns
  happen there, and so do tool calls and their permissions.
- **`deepseek-harness` is never forked.** It is consumed unmodified. Everything
  Athenaeum-specific is a patch-layer bundle in `athenaeum_agent`. If something cannot be
  done with a documented extension point, that is a documented gap and a conversation with
  the harness, not a patch here.

## The rules that keep the seams honest

1. **One bridge, one API.** The desktop, the CLI, the agent bundle, the MCP server, plugins
   and any future connector reach the local knowledge through the **versioned bridge**
   (protocol v1), and reach the hosted Library through **Library API v1**. None of them
   keeps its own graph, parses vault files for knowledge, or opens `knowledge.db`.
2. `athenaeum_agent`, `athenaeum_integrations` and the harness **never** read
   `knowledge.db`, import Athenaeum's crates, or parse vault files for knowledge. They go
   through the bridge.
3. Athenaeum **never** executes coding tools, runs shell commands on the agent's behalf, or
   treats a knowledge request as permission to touch the filesystem, network or Git. Agent
   turns and coding-tool permissions are **not** part of the bridge.
4. Retrieved knowledge crossing a seam is **reference material, never instruction or
   permission**. The trust wrapper in `athenaeum brief --prompt` and in `context.prompt` is
   not optional decoration.
5. **Accepted knowledge is a page.** A model's proposal lives only in the derived index
   until a person accepts it; accepting writes Markdown. Conversation text never becomes
   permanent knowledge by itself.
6. **The cloud is text only.** Sync sends explicitly shared wiki Markdown and the metadata
   needed for repository identity and revision history. It never sends raw sources,
   code repositories, SQLite databases, embeddings, indexes, agent sessions, logs,
   credentials or other local state, and it never runs as a background process. Sync happens
   when a person asks for it: `athenaeum sync`, the app's sync button, or — only if they chose
   `--auto-sync` when connecting their agents — in the foreground at the end of a turn in their own
   agent session, at most once a minute, only for a linked repository that is out of step. Sync refuses
   unsafe paths and non-wiki local state, checks content hashes, detects a stale head and
   conflicting edits, and never overwrites local work silently. Local use needs no network.
7. **GPL boundary.** Athenaeum is GPL-3.0; the other repositories are MIT. Cross the boundary
   with protocol calls (the CLI/bridge, HTTP), never by copying Athenaeum code into an MIT
   package.

## Retired code

- `crates/athenaeum-providers/` — **removed**. A bootstrap model client from before the
  split; providers belong to `deepseek-harness`.
- **`athenaeum_core` (repository)** — **retired**. It held a second, thinner engine with its
  own index (`.athenaeum/local-core.db`). Its unique behaviour (drafting a proposal from a
  source, refusing to attach evidence to or link accepted pages, looser near-duplicate
  detection, the in-process bridge) now lives in `crates/athenaeum-core` here, and
  `athenaeum import-legacy-core` carries its pending proposals into the one index. The
  repository now holds only a no-logic shim, `athenaeum-core`, that forwards old command
  lines to `athenaeum`. Nothing new may depend on it.
- `athenaeum_agent/crates/` — a retired Rust agent runtime. See that repository.

## What the documents in this repo are

| Document | Status |
| --- | --- |
| **`AGENTS.md`** (this file; `CLAUDE.md` is a link to it) | **Authoritative scope for this repository.** |
| `docs/scope.md` | The same boundary in detail, with the reasoning. Current. |
| `docs/architecture.md` | How Athenaeum is built internally. Current. |
| `docs/porting-llm-wiki.md` | The upstream port: what to copy, adapt, and refuse. Current. |
| `docs/agent-integration.md` | The bridge contract, Athenaeum's half. Current. |
| `docs/releasing.md` | How a push to `main` becomes installers and an auto-update. Current. |
| `docs/installing.md` | The one install, `athenaeum setup`, what connecting agents changes, and how to remove it. Current. |
| `docs/portable-format.md` | The portable repository format and the Library sync protocol. Current; the status banner in it says exactly what is built. |
| `docs/vision/build-plan.md` | **Cross-repo product vision. NOT this repository's backlog.** |

`docs/vision/build-plan.md` describes the whole product across all repositories — agent loop,
model roles, tool runtime and all. **Most of it is deliberately not built here.** Read it for
direction; take your scope from this file.

## Commands

```sh
npm install && cargo build          # Rust 1.88+, Node 20+
npm run dev                         # Vite dev server
npm run tauri dev                   # the app
npm run typecheck                   # frontend types
cargo build --release -p athenaeum-cli   # the `athenaeum` CLI and engine, no window needed
npm run build                       # typecheck + bundle
```

**This repository holds no tests.** They live in `athenaeum_validation`, which copies the
repositories into a scratch directory, restores the preserved suites and runs them:
`python3 verify_all.py --list`, then `--only <step>`. See that repository's README for the
steps and their prerequisites.

`@athenaeum/protocol` is consumed from `athenaeum_protocol`. The pinned git revision in
`package.json` is what a release uses; a checkout beside the other repositories verifies
against the sibling (the validation runner links it).

Linux needs the webview headers first:
`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`, plus
`protobuf-compiler` and `libprotobuf-dev` (the well-known `.proto` types) for LanceDB. The
`athenaeum` CLI and `athenaeum-core` need none of it.

## Conventions

- The frontend is React 19 + Tailwind 4 + zustand, ported from llm_wiki. Match the
  surrounding style; `@/` resolves to `src/`.
- `athenaeum-core` has no UI dependency, no Tauri dependency, and makes no network calls.
  The HTTP client for the Library lives in the CLI. Keep it that way — it is what makes the
  platform bundles identical and lets the knowledge model be tested keyless.
- Vault paths are normalized to forward slashes on the way in, so a knowledge base created
  on one platform opens unchanged on another.
- SQLite is compiled from source and linked statically; TLS is rustls, never native-tls.
- Project data lives in `.athenaeum/` inside the project — one directory, shared by the
  vault's metadata and the knowledge index.
- Files end with exactly one trailing newline.

## Licence

Athenaeum is **GPL-3.0**, because its UI and wiki engine derive from
[llm_wiki](https://github.com/nashsu/llm_wiki) (GPL-3.0, © Yong Su). See
`docs/provenance.md`. `athenaeum_agent`, `athenaeum_integrations`, `athenaeum_protocol`,
`athenaeum_api`, `athenaeum_library`, `athenaeum_admin` and `deepseek-harness` are MIT —
separate repositories in separate processes, communicating over the bridge and the Library
API. Do not copy Athenaeum code into any of them.
