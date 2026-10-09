# Scope

[`AGENTS.md`](../AGENTS.md) states the boundary. This document explains why it is where it
is, and what happens at each edge.

## Why this document exists

Athenaeum, `athenaeum_agent` and `deepseek-harness` all carried the same 3,088-line
product specification. A coding session opening any one of them read the whole product and
started building whichever section it landed on — agent loops appeared in the knowledge
library, knowledge storage appeared in the agent layer. The specification was never wrong;
it simply described three repositories at once while sitting inside each of them.

The fix is that each repository states its own scope in a file the session reads first,
and the shared specification is filed as vision rather than backlog.

## The repositories

The product is one application assembled from independently useful pieces. [`AGENTS.md`](../AGENTS.md)
has the table; the reasoning is this.

**`athenaeum`** is the application a person opens, **and** the one local engine behind it. The
desktop app, the `athenaeum` CLI, the agent bundle, the MCP server and every future connector are
clients of that single engine, so there is one knowledge graph, one index and one answer to "what
does this project know". It was once described as only the "visible layer", with a second engine in
a separate `athenaeum_core` repository. The product owner reversed that: two engines meant two
indexes that could disagree, which is the failure this scope exists to prevent. `athenaeum_core` is
retired to a forwarding shim (see [Retired code](#retired-code)).

**`deepseek-harness`** is a general coding agent. It knows nothing about Athenaeum and never will.
It is consumed **unmodified upstream** — never forked, never patched in place. Everything
Athenaeum-specific reaches it as a bundle.

**`athenaeum_agent`** is that bundle: a capability seam that teaches the harness one new thing,
*the project has a knowledge base and here is how to ask it questions*. It holds no knowledge
itself. It is the "Codex" of the product: the coding-agent half, with the harness beneath it.

**`athenaeum_integrations`** is the same seam for everyone else: an MCP server and a typed client,
with ready-made configuration for other agents. Reads are always allowed; every write is an explicit
grant.

**`athenaeum_protocol`** owns the wire shapes the others share, and their fixtures, so that a Rust
client and a TypeScript server cannot silently disagree.

**`athenaeum_api`**, **`athenaeum_library`** and **`athenaeum_admin`** are the hosted side: a
text-only service and two static web frontends. The Library is a GitHub-like place to publish,
browse, version and sync a person's accepted wiki memory. It stores Markdown and the minimal
metadata for repository identity and revision history. It does no graph processing and no model
work, and it never receives raw sources, databases, indexes, embeddings, sessions or credentials.

**`athenaeum_validation`** holds the product tests that cross repositories, and the preserved
suites of the others.

The split survives because each piece degrades cleanly. The harness with no bundle is an ordinary
coding agent. The bundle with no knowledge base is a no-op. Athenaeum with no agent is a knowledge
app you edit by hand. Athenaeum with no network is a complete local app; the Library is optional
and always explicit.

## What crosses the seam

Locally, one thing: **the bridge**, protocol v1 — `athenaeum bridge` as JSON over stdio for
other processes, and the same function called in-process by the desktop app. Versioned, storage
independent, documented in [`agent-integration.md`](agent-integration.md). To the hosted Library,
one other thing: **Library API v1** over HTTPS, which carries text and nothing else
([`portable-format.md`](portable-format.md)). The agent's turns, tool calls and permissions are
carried by neither.

The runtime knows it can ask for knowledge. It does not know Athenaeum is written in Rust,
stores in SQLite, indexes with FTS5, or keeps evidence in content-addressed blobs. That
ignorance is the feature — it is what lets either side be rewritten without touching the
other.

Two prohibitions hold the seam:

- **Nothing outside Athenaeum reads `knowledge.db`, imports `athenaeum-core`, or parses
  vault files directly.** A convenient direct read is how a seam dies.
- **Athenaeum never acts on the agent's behalf.** A knowledge request is a question, not
  a grant of filesystem, shell, network or Git permission. Athenaeum has no coding tools
  to lend even if it wanted to.

## Where knowledge lives

Markdown files on disk are **the source of truth**. `wiki/*.md` with YAML frontmatter and
`[[wikilink]]` cross-references, `raw/sources/` holding immutable originals, `index.md` as
the catalog, `log.md` as the chronological record, `purpose.md` and `schema.md` as the
wiki's intent and rules.

This is Karpathy's LLM Wiki layout, and it is chosen for one reason: **the vault opens in
Obsidian**. A person can read, edit and link their knowledge with tools that already exist
and will outlive this project. The agent edits it with ordinary file tools. Nothing is
locked inside a database.

The SQLite knowledge library is a **derived index over that vault**. It holds nodes, typed
edges, evidence, authority, provenance, history and full-text search, and it answers the
questions markdown cannot answer quickly: traversal, shortest paths, bidirectional
navigation, knowledge briefs, staleness.

The consequences are worth being explicit about:

- The index is **rebuildable from the vault at any time**. Deleting `knowledge.db` must
  never lose knowledge. If it can, something was stored in the wrong layer.
- Edits made outside the app — in Obsidian, in an editor, by the agent — are legitimate
  and must reconcile into the index rather than being treated as corruption.
- Knowledge that genuinely has no markdown representation (the utility journal, derived
  graph metrics, index bookkeeping) may live only in SQLite, because losing it on rebuild
  costs nothing.

## Model use inside Athenaeum

Athenaeum may call a model directly for **ingest**: reading a source, analysing it, and
generating or updating wiki pages. That is wiki building, and it is Athenaeum's job.

Athenaeum must never grow a **tool-calling agent loop**. The line is not "does it call a
model" but "does it run a loop where a model requests tools and Athenaeum executes them."
The moment that appears, the split has collapsed and there are two agent runtimes to
maintain.

Chat in the UI is a **client** of `athenaeum_agent`. The user types, Athenaeum forwards,
the harness runs the turn, Athenaeum renders the stream. The conversation state, tool
calls, permissions and session belong to the harness.

## Retired code

**`crates/athenaeum-providers/`** was a model client with per-role routing, written before
the runtime was split out. It has been **removed**: providers belong to `deepseek-harness`.

**`athenaeum_core` (the repository)** held a second, thinner knowledge engine with its own index,
`.athenaeum/local-core.db`. It is **retired**: its unique behaviours were ported into
`crates/athenaeum-core` here (drafting a proposal from a source, refusing to attach evidence to or
link accepted pages, looser near-duplicate detection, the bridge as an in-process function), and
`athenaeum import-legacy-core` carries its pending proposals into `knowledge.db`. What remains
there is a shim that forwards the old `athenaeum-core` command line to `athenaeum`.

**`athenaeum_agent/crates/`** is a complete coding-agent runtime in Rust — 335 passing
tests — written when no readable agent implementation was available to reuse. DeepSeek
Harness now fills that role. It still builds, and it is retired: no new behaviour, no
extensions, no new callers. See that repository's `docs/runtime-pivot.md`.

## The vision document

[`vision/build-plan.md`](vision/build-plan.md) is the full product specification. It is
accurate about the destination and says nothing about which repository builds what, because
it predates the split.

Sections describing the agent loop, model providers, tool runtime, permissions, sessions,
streaming and context compaction are **implemented in `deepseek-harness`**, not here.
Sections describing knowledge types, the graph, evidence, authority, staleness, retrieval
and the Obsidian-style interface are Athenaeum's.

Code comments cite it as `§N`. Those are references to the specification, not open tickets.
