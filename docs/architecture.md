# Architecture

## The one decision everything else follows from

The knowledge library is a plain Rust library with no UI, no Tauri, no HTTP
client and no model dependency. Everything else — the CLI, the desktop shell,
eventually the agent — is a consumer of it.

```text
                    ┌──────────────────────┐
                    │   athenaeum-core     │   the product
                    │                      │
                    │  knowledge model     │   no UI
                    │  SQLite persistence  │   no network
                    │  graph navigation    │   no model
                    │  authority + trust   │
                    └──────────┬───────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
      athenaeum-cli      src-tauri +        (next: the
      the `athenaeum`    src/ — desktop      agent loop)
      command            shell
                               │
                               ▼
                    ┌──────────────────────┐
                    │ athenaeum-providers  │
                    │ Anthropic / OpenAI / │
                    │ Google / Ollama / …  │
                    └──────────────────────┘
```

Three things fall out of it, and they are the reasons for the arrangement:

**The platforms cannot drift.** The part that holds knowledge is byte-identical
on Windows, macOS and Linux. SQLite is compiled from source and linked
statically rather than borrowed from the host, TLS is rustls rather than
whatever OpenSSL the machine has, and anchored paths are normalized to forward
slashes. What differs between platforms is confined to four small
`tauri.*.conf.json` files.

**The knowledge outlives the model.** Switching providers touches nothing in
`athenaeum-core`. Losing access to a provider entirely does not make an
existing knowledge base less readable.

**It is testable without an API key.** All 148 tests run offline, in under two
seconds, including the end-to-end acceptance test for the product thesis.

## Why the graph view is not here yet

The build plan is explicit that visualization is not the product (§33), and
that the order of work is knowledge model → persistence → relationships →
navigation → retrieval quality → UI. A beautiful graph over an empty database
demonstrates nothing.

So the shell shows lists, detail panes and briefs. It is deliberately plain.
The graph gets drawn once there is something in it worth looking at.

## The knowledge model

A node is a **semantic object**, never a chunk of text (§9). `AuthService` is a
node; "bytes 4000–4500 of AuthService.ts" is not. Nodes hold meaning; source
files remain the source of truth for implementation.

| Concept | Where | Purpose |
| --- | --- | --- |
| `KnowledgeNode` | `model/node.rs` | A meaningful claim, with type, authority, status and provenance |
| `KnowledgeEdge` | `model/edge.rs` | A typed relationship, itself a first-class object |
| `Evidence` | `model/evidence.rs` | Why we believe a node |
| `SourceAnchor` | `model/anchor.rs` | A reference into source meant to survive refactoring |
| `NodeVersion` | `model/version.rs` | What a node used to say |

Some deliberate choices worth knowing about:

**Nothing is true because a model said so.** A new node defaults to
`UnverifiedObservation` authority; a new edge defaults to `Proposed` status and
is not traversed until something promotes it. Deterministic edges — read out of
imports, an AST, git history — arrive `Valid`, because no model judgement was
involved.

**Edges are stored once and read both ways.** Arriving at a test via
`verified_by`, you can walk back out along `verifies`. That inverse is computed
from a table in `EdgeType::inverse()` rather than by writing a second row,
because two rows describing one relationship eventually disagree. A test
asserts the inverse mapping is involutive.

**Staleness is not falsehood.** A node can be high-confidence and stale at the
same time, so status and authority are separate axes. Superseded knowledge
stays in the database and stops being offered — history is usually the valuable
part.

**Authority is contextual.** `authority::resolve` ranks conflicting claims by
authority, then by scope specificity, then by recency. That middle term is what
makes the plan's own example come out right: a project rule saying "this
project uses SQLite" beats a global preference for PostgreSQL, not because it
is a stronger kind of claim, but because it is about the thing in front of you.

**Reading knowledge never rewrites it.** Retrieval, use and citation counts go
to an append-only journal and are derived on demand, so a node's canonical
content is untouched by having been read.

## Retrieval: navigate, don't rank

The primary operation is a walk, not a top-k query.

```text
task text
   ↓
search  ─────────────▶ seed nodes        (lexical; candidates, not answers)
   ↓
navigate ────────────▶ reachable nodes   (breadth-first, under a budget)
   ↓
brief ───────────────▶ summaries, relationships, anchors, warnings
```

An integration test pins the difference from plain retrieval: `Decision_042`
contains no occurrence of the word "OAuth", so no amount of lexical or semantic
similarity to the task finds it. The relationship does.

Search is a seed-finder, and it widens deliberately: it first requires every
term, and falls back to any-term matching when nothing matches all of them —
because a brief is seeded from a task sentence, and no node contains all of
"fix", "the", "oauth" and "bug". Both behaviours are pinned by tests.

Briefs are budgeted. A test adds 200 unrelated nodes and asserts the brief for
an existing task does not grow, because context efficiency is the entire value
proposition.

## The trust boundary

Anything that reaches the graph may be untrusted: READMEs, code comments, git
history, model output. `KnowledgeBrief::to_prompt()` wraps retrieved knowledge
in an explicit statement that it is reference material — that it does not
override system instructions, grant permissions, or authorize tool execution.
A README saying "ignore previous instructions" is repository data, and stays
repository data.

## Model roles

`athenaeum-providers` addresses models by role rather than by name (§5, §6):

| Role | Does | Typically |
| --- | --- | --- |
| `Primary` | Plans, implements, debugs | An expensive model |
| `Wiki` | Navigates the graph, writes briefs | A cheap model |
| `Curator` | Turns discoveries into nodes and edges | A cheap model |
| `Verifier` | Adjudicates high-consequence claims | Mid or expensive |

Only `Primary` is required; the rest fall back to it, so a user with one model
configured gets a working system — just one paying primary prices for graph
navigation, which `is_explicitly_assigned` lets the UI point out. Roles can
span providers: GPT primary with a Claude wiki agent is a configuration, not a
fork.

## Storage

```text
.athenaeum/
├── knowledge.db     nodes, edges, evidence, node_versions, journal, nodes_fts
├── blobs/sha256/    content-addressed evidence, fanned out by hash prefix
├── exports/
└── cache/
```

Canonical data is nodes, edges, evidence and history. The full-text index is
derived and can be dropped and rebuilt from the nodes table at any time —
there is a test that deletes it and rebuilds it. Schema changes go through an
append-only migration list; a database written by a newer build is refused
rather than silently downgraded, because a downgrade that drops columns
destroys the user's knowledge.

Timestamps are RFC 3339 text and enums are lowercase strings, so the file stays
readable in any SQLite browser. That costs a few bytes per row and is worth it:
the knowledge belongs to the user.

## Where this sits against the build plan

| Phase | State |
| --- | --- |
| 0 — Foundation: config, SQLite, schema, journal, CLI, provider abstraction | **Done** |
| 1 — External model agent: agent loop, tools, permissions, sessions | Not started |
| 2 — Repository understanding: file/symbol/AST indexes, git history | Not started |
| 3 — Core: nodes, edges, evidence, provenance, authority, anchors | **Done**, minus the curator |
| 4 — Wiki agent: navigation, briefs, role configuration | Navigation, briefs and roles done; the agent that drives them is not |
| 5 — Agent + wiki integration | Not started — this is the milestone that matters |
| 6 — Verification: anchor relocation, staleness, contradictions | Model and detection in place; relocation is not |
| 7 — Obsidian-style app | Deliberately minimal |

Phase 5 is the real product milestone, and the honest summary of this base app
is that it builds the floor Phase 5 stands on. The knowledge side is done well
enough to hold a session's discoveries; what is missing is the loop that puts
them there automatically.

## Next

1. **The agent loop** (Phase 1) — tool calling, permissions, session state,
   driven through `athenaeum-providers`.
2. **The curator** (Phase 3) — entity resolution and duplicate detection so
   sessions attach evidence to existing nodes instead of spawning
   near-duplicates. `find_node_by_title` is the hook it plugs into.
3. **Repository understanding** (Phase 2) — deterministic extraction of
   imports, symbols and git history. These produce `Valid` edges directly and
   are the cheapest real knowledge available.
4. **Anchor relocation** (Phase 6) — `AnchorResolution` describes the outcomes;
   nothing implements the search yet.
