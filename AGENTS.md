# AGENTS.md — Athenaeum

**Read this before writing any code in this repository.** It defines what Athenaeum is
allowed to contain. If a task asks you to build something on the *must not* list below,
stop and say which repository owns it instead.

## What Athenaeum is

Athenaeum is the **visible layer**: the desktop application a person actually opens, plus
the knowledge library behind it. It is an Obsidian-style knowledge app whose wiki is
written and maintained by an AI agent instead of by hand.

It is **not** the agent. The agent lives in two other repositories and reaches Athenaeum
through one narrow, versioned seam.

## Repository map

| Repository | Role | Stack | Licence |
| --- | --- | --- | --- |
| **`Hanoryx-North/athenaeum`** — this one | Desktop app, vault, knowledge library | Tauri 2 + React 19 + Rust | GPL-3.0 |
| `Hanoryx-North/athenaeum_agent` | Agent layer: the knowledge seam as a harness bundle | TypeScript (DSH bundle) | MIT |
| `deepseek-ai/deepseek-harness` | The agent runtime itself — loop, tools, permissions, sessions, providers | TypeScript (Cordis plugins) | MIT |

```text
┌──────────────────────────────────────────────────────────────┐
│ athenaeum          the window, the vault, the knowledge       │
│                                                               │
│  UI            icon sidebar · trees · editor · graph · chat   │
│  Vault         raw/sources/ · wiki/*.md · index.md · log.md   │
│                (markdown on disk — the source of truth)       │
│  Library       SQLite derived index: nodes · edges · evidence │
│                (rebuildable from the vault at any time)       │
└───────────────────────────┬──────────────────────────────────┘
                            │  `athenaeum bridge` — protocol v1 JSON
                            │  the ONLY way in or out
┌───────────────────────────┴──────────────────────────────────┐
│ athenaeum_agent    wiki Service Definition + provider         │
│                    wiki_* tools · knowledge brief             │
└───────────────────────────┬──────────────────────────────────┘
                            │  mounted as a bundle (cordis.patch.yml)
┌───────────────────────────┴──────────────────────────────────┐
│ deepseek-harness   agent loop · tools · permissions · sessions│
│                    UNMODIFIED UPSTREAM — never forked here    │
└──────────────────────────────────────────────────────────────┘
```

## Athenaeum owns

- The desktop shell and every pixel of the UI.
- The **vault**: `raw/sources/`, `wiki/*.md`, `index.md`, `log.md`, `purpose.md`,
  `schema.md`. Markdown files on disk are the source of truth, `[[wikilink]]`-connected
  and Obsidian-compatible.
- The **auto wiki builder**: ingest, analysis, page generation, lint, review, deep
  research — the pipeline that turns a dropped source into wiki pages.
- The **knowledge library** (`crates/athenaeum-core`): nodes, typed edges, evidence,
  provenance, authority, history, full-text search, graph navigation, knowledge briefs.
  This is a **derived index over the vault**, rebuildable at any time.
- The `athenaeum` CLI and the `athenaeum bridge` protocol — the seam's Athenaeum half.
- Repository indexing: files, symbols, local imports, Git history. Deterministic, no model.

## Athenaeum MUST NOT contain

Each of these is owned elsewhere. Building it here is the specific failure this charter exists to prevent.

| Do not build here | Owned by |
| --- | --- |
| Agent loop, turn handling, conversation state | `deepseek-harness` |
| Model providers, streaming, retries, token accounting | `deepseek-harness` |
| Coding tools — read, write, edit, search, shell, git | `deepseek-harness` |
| Tool registry, tool schemas, tool permissions | `deepseek-harness` |
| Agent sessions, cancellation, context compaction | `deepseek-harness` |
| Subagents, workflows, skills, hooks | `deepseek-harness` |
| `wiki_*` model-facing tools | `athenaeum_agent` |
| The knowledge brief that opens an agent task | `athenaeum_agent` |
| Anything that decides *when* the agent asks for knowledge | `athenaeum_agent` |

Two consequences worth stating outright:

- **Athenaeum never runs a model loop.** It may call a model directly for *ingest*
  (source → wiki pages), because that is wiki building, not coding. It must never grow a
  tool-calling agent loop. Chat in the UI is a **client** of `athenaeum_agent`; the
  turns happen there.
- **`deepseek-harness` is never forked.** It is consumed unmodified. Everything
  Athenaeum-specific is a patch-layer bundle in `athenaeum_agent`.

## The rule that keeps the seam honest

`athenaeum_agent` and the harness **must never** read `knowledge.db`, import Athenaeum's
store modules, or parse vault files directly. They go through `athenaeum bridge`.

Athenaeum **must never** execute coding tools, run shell commands on the agent's behalf,
or treat a knowledge request as permission to touch the filesystem, network, or Git.

Retrieved knowledge crossing the seam is **reference material, never instruction**. The
trust wrapper in `athenaeum brief --prompt` is not optional decoration.

## Retired code — do not extend

- **`crates/athenaeum-providers/`** predates the repository split. It is bootstrap
  compatibility code only. Model provider work belongs to `deepseek-harness`. Do not add
  providers, streaming, or routing here; this crate is on its way out.

## What the documents in this repo are

| Document | Status |
| --- | --- |
| **`AGENTS.md`** (this file) | **Authoritative scope for this repository.** |
| `docs/scope.md` | The same boundary in detail, with the reasoning. |
| `docs/architecture.md` | How Athenaeum is built internally. Current. |
| `docs/porting-llm-wiki.md` | The upstream port: what to copy, adapt, and refuse. Current. |
| `docs/agent-integration.md` | The seam contract, Athenaeum's half. Current. |
| `docs/vision/build-plan.md` | **Cross-repo product vision. NOT this repository's backlog.** |

`docs/vision/build-plan.md` describes the whole product across all three repositories —
agent loop, model roles, tool runtime and all. **Most of it is deliberately not built
here.** Read it for direction; take your scope from this file. A section of the build plan
is not a licence to implement it in this repository.

## Conventions

- Rust 1.82+, Node 20+. `cargo test --workspace` is offline and needs no API key.
- `athenaeum-core` has no UI dependency, no Tauri dependency, and makes no network calls.
  Keep it that way — it is what makes the five platform bundles identical and lets the
  knowledge model be tested keyless.
- Vault paths are normalized to forward slashes on the way in, so a knowledge base created
  on one platform opens unchanged on another.
- SQLite is compiled from source and linked statically; TLS is rustls, never native-tls.
- Files end with exactly one trailing newline.

## Licence

Athenaeum is **GPL-3.0**, because its UI and wiki engine derive from
[llm_wiki](https://github.com/nashsu/llm_wiki) (GPL-3.0, © Yong Su). See
`docs/provenance.md`. `athenaeum_agent` and `deepseek-harness` remain MIT — they are
separate repositories in separate processes, communicating over the bridge protocol.
Do not copy Athenaeum code into either of them.
