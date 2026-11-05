> Current source layout and root commands: [the consolidation guide](../../../docs/consolidation.md).
> Older sibling-checkout and pnpm development examples below are historical.

# Pages and nodes: what the vault means for the seam

> **Status: page pointers are built; the direct-edit guidance below is historical.** The bridge
> and this bundle expose a page path, but model-facing knowledge tools now leave new knowledge as
> a proposal for a person to accept. Ordinary harness file tools can still write `wiki/` if granted
> that permission. Desktop agent mode cannot guarantee person review until a harness permission
> policy or documented extension protects those writes. The harness is not patched here.

Athenaeum's knowledge model changed. Markdown on disk is now the source of truth: an
Obsidian-compatible vault of `wiki/*.md` with frontmatter and `[[wikilinks]]`, plus
`purpose.md`, `schema.md` and `raw/sources/`. SQLite is a derived index over it, held on
Athenaeum's side to a blunt acceptance test — *delete `knowledge.db` and you lose nothing but
time.*

Protocol v1 was node-centric and knew nothing about pages, which left an agent that was meant to
maintain the vault with no way to find the file behind a node.

## The problem, as it stood

A vault page becomes a node whose id is `vault_page_{hash(vault-relative path)}` — a one-way hash,
so the path cannot be recovered from the id. `KnowledgeNode` has no path field. The only place a path
surfaced was `evidence`: the indexer attaches one `Documentation` record per page, anchored at the page
file. `WikiBrief.anchors` carries citation strings too, but it is a flat list for the whole brief,
unattributed to any node and truncated to twelve, so it could not answer "where does *this* node
live".

## The decision

**When a node is a page on disk, the seam returns that page's path, and the agent reads and
edits it with its ordinary file tools. The seam does not proxy page content, and this
repository grows no `wiki_read_page` or `wiki_write_page`.**

The case for it is the one this project keeps having to relearn. The harness already has
read, write and edit tools better than anything this bundle would write; a `wiki_*` tool
wrapping a file read is the overlap this repository exists to avoid; and an agent editing
pages directly is how the vault is meant to be maintained, by the same route a person editing
in Obsidian takes.

The case against is more interesting, and it is what the conditions below are made of.

### What the path does not replace

`wiki_open` keeps returning the node's content, and keeps returning its relationships,
evidence, history and utility. **None of those are in the markdown.** They are the reason the
knowledge base exists. An agent that reads the file instead of opening the node gets the prose
and loses the graph, which is a worse outcome than having no seam at all. The path is an
addition to what a node carries, never a substitution for it, and the tool descriptions have
to say which job each one does: the node tells the agent what the project knows, the path
tells it where to change what the project says.

Keeping content on the wire also keeps the boundary rule intact. The moment a consumer here
needs a summary and has only a path, it parses vault markdown — which [AGENTS.md](../../../AGENTS.md)
forbids, for the reason that ends the seam the first time it happens.

### The path has to be absolute, and Athenaeum has to produce it

A vault-relative `wiki/concepts/attention.md` is useless to the agent's file tools unless
something resolves it against the right root, and there are three plausible roots that need
not agree: `wiki-athenaeum`'s configured `workspace` (which defaults to the runtime's working
directory), Athenaeum's project root (the nearest ancestor holding `.athenaeum/`), and the
harness sandbox's `workspaceRoot`.

**This repository must not do that resolution.** Joining a path against a root it guessed is
precisely the "knowing the vault layout" the seam rule forbids, and it is the case this side
would get wrong. Athenaeum knows where the vault root is. Athenaeum returns the absolute path.

Where the vault sits outside what the agent's tools can reach, the honest answer is to return
the path anyway and let the harness's own sandbox refuse the read. A seam that silently
withholds a path it has is harder to debug than a tool call that fails loudly.

### Renaming is the dangerous part, not editing

The node id is a hash of the page's path. Editing a page keeps its id; **moving or renaming
one changes it.** The indexer then removes the departed node and creates a new one, and the
utility history, the model-proposed edges and everything else attached to the old id are
attached to a node that no longer exists.

Before the seam named pages the agent could not rename one, because it did not know where any
page was. Handing it a path and telling it to use ordinary file tools makes `mv` a
knowledge-destroying operation that looks like an ordinary refactor. This is the single strongest
argument against the decision, and it is not an argument against paths — it is a requirement that
Athenaeum carry node identity across a rename (request 5 in
[the table below](#what-athenaeum-did-about-it)). That is not built, so the tool guidance says
plainly: edit the page, do not move it.

### Not every node is a page

The store holds three populations: nodes derived from the vault, nodes from the repository
scan (`athenaeum index` — files, symbols, imports, local git history), and nodes a model or a
user created over the bridge. Only the first has a page. The field is therefore optional, every
consumer degrades when it is absent, and the model is never taught that nodes have files.

### `curation: false` stops meaning what it meant

`tool-wiki`'s `curation` switch is how a deployment says the model may consult knowledge but
not write it. Once the agent edits pages with the harness's write tool, that switch governs
nothing: filesystem permission does.

This is worth saying out loud rather than quietly leaving in place. It was already only
half-true — the vault is files inside the workspace, and a coding agent with write access could
always have edited them. What changes is that the seam would now be *pointing at them*, which
turns an accident into an affordance. A deployment that needs knowledge to be genuinely
read-only has to say so to the sandbox, not to `tool-wiki`; [docs/agent-layer.md](agent-layer.md)
says so too.

## What Athenaeum did about it

All of it is additive, and protocol v1 requires consumers to ignore fields they do not understand, so
none of it needed a version bump. The wire contract is in Athenaeum's
`docs/agent-integration.md`; the vendored fixtures here pin it.

| Request | Outcome |
| --- | --- |
| **1. An optional `page` on a node** | **Built**, on `open`'s `detail` and not on every node a search or a walk returns. `{ path, absolutePath, contentHash }`. Absent — not `null` — for anything that is not a page. The absolute path is resolved by the library, which is what keeps this repository out of the vault layout. |
| **2. The same on brief nodes** | **Built.** The brief's prompt also states the location as a plain `Page:` line under the node, inside the trust wrapper, as a fact and not an instruction. |
| **3. Tell "not a page" from "an older library"** | **Built** as `capabilities.features`, which lists `page_pointers` and `vault_sync`. A library that predates the list omits it. |
| **4. Staleness** | **Built** as `vault_sync`. Before the bridge answers a read it compares a metadata-only fingerprint of the vault with the one recorded at the last index and reconciles only when they differ, so a page the agent just edited, added or deleted is what the next question sees. An unchanged vault costs one directory walk (+4 ms at 1,000 pages, +55 ms at 10,000). It keeps an index that exists and never builds the first one: that stays `athenaeum index-vault`, because a first index of a large vault outlasts what a waiting caller should be made to wait. |
| **5. Identity across a rename** | **Not built, on purpose.** Matching a departed path to an arriving one by content hash and carrying the node id over would make the index depend on the history of the vault, which breaks *the index is a function of the vault* — a store that watched the vault being written and one that saw it once would hold different ids. Until that trade is revisited, moving a page is still a knowledge-destroying operation that looks like an ordinary refactor, and the guidance says so. |

Two related library changes came out of building this:

* **A page cannot be changed through `update`.** Its text is the markdown and the index follows the
  vault, so an edit made to the node would be discarded by the next reconcile. The library refuses it
  with `invalid_request`, naming the file. This bundle registers no `wiki_update` tool, so it is
  unaffected, but the refusal protects every other client.
* **Search finds a node named by an identifier from the words a person would type.** `oauth callback`
  now reaches `OAuthCallback`. A page title is very often an identifier, and a model phrases a query in
  words.

## What this repository did

* The protocol types carry `WikiPage`, the optional `features`, and the page on a brief node.
* `wiki_open` returns the page, with the absolute path when the library supplied one and the relative
  path otherwise, and renders it as a `Page:` line. The content hash stays out of what the model reads,
  because a digest only costs context.
* The `wiki_open` description and the system-prompt section now tell the model to propose page
  changes for human review and avoid writing, moving or renaming the file without explicit
  permission. Both are conditional on a node actually naming a page.
* The brief needed no change: it injects the library's prompt verbatim, and that prompt now carries the
  `Page:` lines.
* `curation` keeps the meaning it had, with the caveat below.

## What stays true either way

This repository still stores no knowledge, caches nothing, and does no graph logic. A path is not
a cache — it is a pointer the library produced, forwarded unchanged, valid only as long as the
answer that carried it. Nothing here resolves it, joins it, watches it, or reads what is behind
it.
