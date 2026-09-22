# Architecture

## The one decision everything else follows from

The knowledge library is a plain Rust library with no UI, no Tauri, no HTTP
client and no model dependency. The CLI and desktop shell consume it locally;
the separately built agent runtime consumes its versioned Wiki contract.

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
              ┌────────────────┼─────────────────┐
              ▼                ▼                 ▼
      athenaeum-cli      src-tauri +       Wiki protocol v1
      the `athenaeum`    src/ — desktop    typed + JSON bridge
      command            shell                   │
                                                  ▼
                                      ┌──────────────────────┐
                                      │ external Repository 2│
                                      │ agent · providers ·  │
                                      │ tools · permissions  │
                                      └──────────────────────┘
```

The boundary is specified in [agent-integration.md](agent-integration.md).
Repository 2 never reaches through it into `KnowledgeStore` or SQLite.

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

**It is testable without an API key.** The full test suite runs offline,
including the end-to-end acceptance test for the product thesis, repository
extraction, anchor relocation and protocol fixture compatibility.

## The vault and its index

The vault — `wiki/*.md`, `raw/sources/`, `purpose.md`, `schema.md` — is the
source of truth. It is plain markdown on disk, it opens in Obsidian, and the
agent edits it with ordinary file tools. Everything `athenaeum-core` stores
about it is a **derived index**, and the test of that claim is blunt:

> Delete `.athenaeum/knowledge.db` and you lose nothing but time.

`crates/athenaeum-core/src/vault/` builds it. A page becomes a typed node,
a `[[wikilink]]` becomes a deterministic edge, a file under `raw/sources/`
becomes a source node, and a page's `sources:` frontmatter becomes evidence
anchored at the raw file. Frontmatter carries the page's type and authority;
where a page predates the convention, its directory and its filename supply
them rather than the page being refused.

No model runs in that pass. Every node and edge it writes is marked
deterministic, which is why they are traversable on arrival instead of
queued as proposals the way a model's suggestions are.

Indexing is a **reconcile, not a rebuild**. Edits arrive from the app's
editor, from Obsidian and from the agent, and all three are legitimate, so
each pass computes what the vault implies, compares it with what the store
holds, and writes only the difference. A pass over an unchanged vault writes
nothing at all. The indexer touches only what it created: a node the user
typed into the knowledge panel and an edge the agent proposed over the
bridge both survive every re-index untouched.

Two properties are load-bearing and tested as such:

* **The index is a function of the vault.** A store that watched the vault
  being written and edited, and a store that saw the finished vault exactly
  once, hold identical knowledge. If they ever disagree, something reached
  SQLite without reaching the markdown, and the fix is to move it to the
  vault rather than to weaken the test.
* **A truncated scan never deletes.** Hitting a scan budget is not evidence
  that the user deleted half their wiki.

The desktop shell reads the result through three commands — `index_vault`,
`knowledge_graph` and `lint_vault` — and `athenaeum index-vault` and
`athenaeum lint` do the same from a terminal. The graph view, the knowledge
tree, search, lint, the review sweep and deep research all draw from that
one projection, so the app holds one description of what is known rather
than one per panel. Layout, filtering and community detection stay in the
view, where the visual design can change without touching the library.

**Lint is part of that projection.** Orphans, broken links and dead ends
are questions about edges, so `vault::lint` answers them from the nodes and
edges rather than by walking `wiki/` a third time; the broken links it
reports are the ones the indexer could not resolve, not a second opinion
about the same markdown. Semantic lint — contradictions, stale claims,
concepts referenced everywhere and written up nowhere — still needs a model,
and it reads the same index for the pages and summaries it sends.

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

## The review queue

Phase 6's acceptance test is that contradictions stay *visible* and that stale
knowledge can be restored rather than silently dropped. `review::sweep` is the
visibility half, and it is deliberately only that half: it reads, and never
rewrites a status, resolves a dispute or deletes anything. Deciding which of
two rival claims is right is a judgement, and judgement belongs to the person
or to the runtime's verifier. Noticing that a decision is owed is mechanical,
so it lives here, where it runs offline and is tested without a model.

| Finding | What it means |
| --- | --- |
| `contradicted` | Marked contradicted, awaiting a decision (§17) |
| `disputed` | Several live claims describe one subject; one governs (§14) |
| `anchor_lost` | Its anchor no longer resolves to anything (§16) |
| `stale` | Its source moved or changed; the claim may still hold (§17) |
| `unsupported` | A rule or decision resting on no evidence (§15) |
| `proposed_edge` | A relationship a model proposed, not yet accepted (§20) |
| `unreachable` | Live, but no traversable edge reaches it (§22) |
| `noisy` | Retrieved again and again, never used (§40, §41) |
| `empty` | No title, or no words at all |

A contradiction is visible however it was recorded — a node marked
contradicted, or an accepted `contradicts` relationship between two claims
still being offered — and the finding names the claim on the other side.
`athenaeum contradict a b` records one and withholds *both* claims until
somebody decides, because which of two disagreeing claims is wrong is exactly
the question nobody has answered yet; picking the older one would be a guess
dressed as a fact. `athenaeum supersede old new` settles it, and the loser is
kept rather than deleted (§17). Once one side is superseded the disagreement
is history, not an open dispute, and the finding goes away.

`disputed` is the §14 worked example made operational. Claims are grouped by
subject — same scope, same type, same title — and `authority::rank` decides
which one an agent is actually being handed. The finding is raised against the
*others*, because those are what somebody has to merge, supersede or re-scope.

Two restrictions keep it honest. Claims at different scopes are not rivals:
"React state is immutable" and "this file mutates a cache deliberately" are
both true, which is why the subject includes scope. And **structure cannot
contradict structure** — the `Error` enum and the `error` module share a
lowercased name and disagree about nothing, so node types that describe things
that *exist* take no part in dispute detection, only types that describe things
somebody *asserted*.

`noisy` is §40's warning measured rather than guessed at. Utility is counted
from the journal on demand and never written back onto a node, so a sweep that
notices a node has been retrieved nine times and used none of them has still
not modified it — reading knowledge does not rewrite it (§41).

Only findings that could hand an agent a wrong answer fail a build. Being
untidy — an unsupported rule, an unreachable node — is reported and forgiven
unless `athenaeum check --strict` says otherwise.

## The trust boundary

Anything that reaches the graph may be untrusted: READMEs, code comments, git
history, model output. `KnowledgeBrief::to_prompt()` wraps retrieved knowledge
in an explicit statement that it is reference material — that it does not
override system instructions, grant permissions, or authorize tool execution.
A README saying "ignore previous instructions" is repository data, and stays
repository data.

## Model roles

The existing bootstrap provider crate addresses models by role rather than by
name (§5, §6):

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
fork. The provider crate predates the two-repository split. Runtime provider
ownership and further development now belong to Repository 2; it connects to
this library through the Wiki protocol, not through this compatibility code.

## Storage

```text
.athenaeum/
├── knowledge.db     nodes, edges, evidence, node_versions, journal, nodes_fts
│                    — derived from the vault; safe to delete
├── blobs/sha256/    content-addressed evidence, fanned out by hash prefix
├── exports/
└── cache/
```

Canonical data is whatever was not derived from the vault: nodes the user
typed, edges the agent proposed, evidence attached by hand. Everything the
vault implies is rebuilt by `index-vault`, and the full-text index is
derived from the nodes table in turn — there is a test that deletes it and
rebuilds it, and another that rebuilds the whole vault-derived graph from
an empty database and compares it against one that was reconciled edit by
edit. Schema changes go through an
append-only migration list; a database written by a newer build is refused
rather than silently downgraded, because a downgrade that drops columns
destroys the user's knowledge.

Timestamps are RFC 3339 text and enums are lowercase strings, so the file stays
readable in any SQLite browser. That costs a few bytes per row and is worth it:
the knowledge belongs to the user.

## Where this sits against the build plan

| Phase | State |
| --- | --- |
| 0 — Foundation: config, SQLite, schema, journal, CLI | **Done** |
| 1 — External model agent: agent loop, tools, permissions, sessions | **Runtime responsibility; intentionally not built here.** DeepSeek Harness serves it |
| 2 — Repository understanding: file/symbol/AST indexes, git history | **Deterministic extraction done:** bounded file scanning, Rust/TypeScript/JavaScript/Python/Go declarations with spans and visibility, imports, calls, implements and test references, local dependency edges and Git modification history. Compiler-grade AST identities and broader language coverage remain. |
| 3 — Core: nodes, edges, evidence, provenance, authority, anchors | **Done**; model-safe curation operations and conservative near-duplicate resolution are exposed |
| 4 — Wiki agent: navigation, briefs, role configuration | Library-side navigation and briefs done; the Wiki agent is harness model configuration |
| 5 — Agent + wiki integration | **Done on both sides:** protocol v1, typed service and CLI bridge here; the `athenaeum_agent` bundle consumes them as a harness capability seam |
| 6 — Verification: anchor relocation, staleness, contradictions | **Deterministic relocation and review done:** exact, symbol, fingerprint and stale outcomes are searched and persisted, and `review::sweep` makes disputes, contradictions, lost anchors, unsupported rules and unaccepted proposals visible; semantic/model revalidation remains external |
| 7 — Obsidian-style app | **Shell ported and wired:** the `llm_wiki` desktop application supplies the UI and the wiki engine, and its graph, knowledge and search panels read the derived index rather than re-deriving one from files. Chat still needs the harness behind it |

Phase 5 was the real product milestone, and it is met without this repository
absorbing runtime responsibilities. The runtime negotiates capabilities,
requests trust-wrapped context, submits model-attributed curation proposals and
reports which knowledge was useful, all without knowing the storage
implementation — over the published protocol, from a different language.

## Next

1. **Deeper structural extraction** (Phase 2 tail) — compiler-grade AST
   identities and additional languages where measured project value justifies
   them. Calls, implements, exports and test references are built; what the
   line-oriented reader cannot see is dispatch through a variable, a name
   reached through an alias, and the difference between two `open`s in
   different modules.
2. **Verification workflows** (Phase 6 tail) — the review queue is built and
   deterministic. What remains is the judgement half: deciding *which* rival
   claim is right, and revalidating a stale claim against changed source.
   Both belong behind the existing protocol and the runtime's verifier
   policy; the local library continues to own persisted outcomes.
3. **Chat on the harness** (Phase 7 tail) — the chat UI is ported and its
   agent-CLI transports are gone; it now needs `athenaeum_agent` behind it,
   driven through the harness SDK. That is the last place where the visible
   layer still has a hole rather than a seam.

## Deterministic repository knowledge

This is the sibling of the vault indexer above, over code rather than
markdown, and it follows the same rules: bounded, read-only, no model, and
reconciled rather than rebuilt.

`athenaeum index` performs a bounded, read-only scan. Generated and private
workspace directories are skipped, non-text and oversized files are not read,
and stable identities make repeated indexing idempotent. File containment,
resolved local imports and Git modification history become `Valid` edges with
deterministic provenance. Removed imports are reconciled and source objects
that disappear are retained as `stale_requires_review`.

The first parser set covers the languages used by this repository plus common
agent projects: Rust, TypeScript/JavaScript, Python and Go. It extracts named
declarations, their visibility, their spans, the names they invoke and the
supertypes they announce. It is not a compiler front end and does not claim to
be; richer language coverage and true AST identities remain Phase 2 work.

### The code graph

A declaration's span is its body, not its signature line, which is both what
its evidence anchor covers and what an anchor fingerprint has to match after
a refactor. Inside that span, an identifier applied to an argument list is a
call site, and the `impl ... for ...`, `implements`, `extends` and base-class
clauses are supertype claims. Neither becomes an edge on the parser's word.

Resolution is where §21 is enforced — *do not connect everything*. A written
name becomes a relationship only when it can be resolved without guessing:
a declaration in the same file, or exactly one declaration in the repository
carrying that name. A name that several declarations answer to resolves to
none of them, because a graph where `open` links to all eleven `open`s is
worse than one where it links to none — the reader cannot tell the difference
between a real relationship and a coincidence of naming. Names the repository
never declares are words, not relationships, which is what keeps standard
library calls out of the graph without a list of them having to be kept.

What survives becomes:

```text
authenticate  ──calls──▶      validate_token      §30
Sqlite        ──implements──▶ Store               §19
auth.test.ts  ──tests──▶      AuthService         §19
```

Test references are recorded once, as `tests` rather than as `calls`: a test
naming the code it exercises is one fact, and describing it with two edges
would be the bloat §40 warns about. Each is recorded at both declaration and
file granularity, because "which function does this cover" and "which tests
cover this file" are different questions. All of them read back the other way
for free — `called_by`, `implemented_by`, `tested_by` — through
`EdgeType::inverse()` rather than a second row.

Two declarations in one file can carry the same qualified name: `from` on two
`impl From` blocks, a helper redefined further down a test file. They are
different declarations, they get different nodes, and their titles carry the
line that tells them apart. The first occurrence keeps the plain name, so the
ordinary case never grows a suffix.

`athenaeum relocate` checks every anchored evidence record. An unchanged
file or region stays exact; a unique named declaration updates by symbol; a
unique identical line region updates by fingerprint; and an unresolved anchor
makes its node stale or review-required without deleting knowledge. Paths are
validated as repository-relative before any read.

Language-neutral v1 fixtures live under
`fixtures/wiki-protocol/v1/`. Tests require every advertised operation to
have a request fixture and pin the capabilities response to the production
vocabulary and budgets.
