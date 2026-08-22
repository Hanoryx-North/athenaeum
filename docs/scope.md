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

## The three repositories

The product is one application assembled from three independently useful pieces.

**`deepseek-harness`** is a general coding agent. It knows nothing about Athenaeum and
never will. It is consumed **unmodified upstream** — never forked, never patched in place.
Everything Athenaeum-specific reaches it as a bundle.

**`athenaeum_agent`** is that bundle: a capability seam that teaches the harness one new
thing, *the project has a knowledge base and here is how to ask it questions*. It is small
on purpose. It holds no knowledge itself.

**`athenaeum`** is the application a person opens and the library that holds what the
project knows. It has a UI; the other two do not.

The split survives because each piece degrades cleanly. The harness with no bundle is an
ordinary coding agent. The bundle with no knowledge base is a no-op. Athenaeum with no
agent is a knowledge app you edit by hand.

## What crosses the seam

One thing: **`athenaeum bridge`**, protocol v1, JSON over stdio. Versioned, storage
independent, documented in [`agent-integration.md`](agent-integration.md).

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

Two subsystems still build and still pass tests, and neither is the direction. They are
documented here so a session does not mistake them for foundations worth extending.

**`crates/athenaeum-providers/`** (this repository) is a model client with per-role
routing, written before the runtime was split out. Provider work belongs to
`deepseek-harness`. Nothing new goes here.

**`athenaeum_agent/crates/`** is a complete coding-agent runtime in Rust — 335 passing
tests — written when no readable agent implementation was available to reuse. DeepSeek
Harness now fills that role. See that repository's `docs/runtime-pivot.md`.

Retired does not mean deleted. It means: no new behaviour, no extensions, no new callers.

## The vision document

[`vision/build-plan.md`](vision/build-plan.md) is the full product specification. It is
accurate about the destination and says nothing about which repository builds what, because
it predates the split.

Sections describing the agent loop, model providers, tool runtime, permissions, sessions,
streaming and context compaction are **implemented in `deepseek-harness`**, not here.
Sections describing knowledge types, the graph, evidence, authority, staleness, retrieval
and the Obsidian-style interface are Athenaeum's.

Code comments cite it as `§N`. Those are references to the specification, not open tickets.
