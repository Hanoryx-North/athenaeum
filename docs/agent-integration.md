# Agent Runtime Integration

Athenaeum is **the app and the one local knowledge engine** (this document calls its
library half "the Wiki Library"). The coding agent is `deepseek-harness`, extended by the
`athenaeum_agent` bundle, in separate repositories, and must remain independently usable when
this library is absent. This repository does not own the agent loop, coding tools, shell, Git,
streaming, context compaction, sessions, or the agent permission system, and none of those
travel over the bridge. **Every client uses this one bridge** — the agent bundle and the MCP
server over `athenaeum bridge`, the desktop app in-process through the same
`athenaeum_core::bridge` function (the Rust path of the `athenaeum-core` crate) — so none of them
needs its own graph or may open `knowledge.db`.

> The old separate `athenaeum-core` command (repository `athenaeum_core`) is retired. The bridge
> is `athenaeum -C DIR bridge`; `athenaeum bridge --workspace DIR` is accepted as the old
> spelling. See [scope.md](scope.md#retired-code).
>
> Below, **"Repository 2"** means the agent runtime: `deepseek-harness` with the `athenaeum_agent`
> bundle (the MCP server in `athenaeum_integrations` is a second, equal consumer of the same bridge).

The integration is intentionally shaped like this:

```text
Agent runtime: deepseek-harness + athenaeum_agent
  Agent loop · providers · tools · permissions · sessions
                    │
                    │ WikiProvider adapter
                    │ protocol v1 JSON
                    ▼
Athenaeum / Wiki Library (this repository)
  context · graph · curation · evidence · history · knowledge.db
```

The runtime knows the versioned Wiki protocol. It does not know that this
implementation uses Rust, SQLite, FTS5, or content-addressed blobs.

## Repository ownership

| Athenaeum — this repository | Agent runtime — `deepseek-harness` + `athenaeum_agent` |
| --- | --- |
| Knowledge nodes and typed edges | Agent loop and conversation messages |
| Evidence, provenance and authority | Model providers and streaming |
| SQLite schema and migrations | Read/write/edit/search tools |
| Graph navigation and briefs | Shell, Git and process execution |
| Retrieval trust wrapper | Workspace and command permissions |
| Knowledge history and utility journal | Cancellation, retries and compaction |
| Wiki protocol implementation | `WikiProvider` interface and `NoWikiProvider` |

Repository 2 must never query `knowledge.db` or depend on Athenaeum's store
modules. Repository 1 must never execute coding tools or infer that a Wiki
request grants filesystem, shell, Git, or network permission.

`crates/athenaeum-providers` has been removed. It was bootstrap compatibility code from
before this repository split, and it was never the runtime boundary. Provider ownership
belongs to the runtime.

## Integration contract

The typed implementation is in `athenaeum_core::wiki`. A consumer can use it
in-process through `WikiService`, or use the language-neutral CLI bridge:

The shared TypeScript and Rust wire types are maintained in the private
`Hanoryx-North/athenaeum_protocol` repository. Desktop dependencies are pinned
to a protocol commit. CI and release builds therefore require an
`ATHENAEUM_PROTOCOL_TOKEN` Actions secret with read access to that repository.

```bash
printf '%s\n' '{
  "protocolVersion": 1,
  "requestId": "knowledge-for-task-1",
  "request": {
    "method": "context",
    "task": "fix the OAuth callback bug"
  }
}' | athenaeum -C /path/to/project bridge
```

PowerShell uses the same wire contract:

```powershell
@'
{
  "protocolVersion": 1,
  "request": { "method": "capabilities" }
}
'@ | athenaeum -C C:\code\project bridge
```

The command writes exactly one JSON response to stdout. A protocol or Wiki
error is also a JSON response and produces a non-zero exit code. Version 1 is
one-shot: Repository 2 may spawn it for each call. A future long-lived JSONL
transport can reuse the same envelopes without changing operation payloads.

Every request has:

```json
{
  "protocolVersion": 1,
  "requestId": "caller-chosen-correlation-id",
  "request": { "method": "capabilities" }
}
```

Successful and failed responses have stable envelopes:

```json
{
  "protocolVersion": 1,
  "requestId": "caller-chosen-correlation-id",
  "status": "ok",
  "response": { "method": "capabilities", "capabilities": {} }
}
```

```json
{
  "protocolVersion": 1,
  "requestId": "caller-chosen-correlation-id",
  "status": "error",
  "error": { "code": "not_found", "message": "..." }
}
```

Call `capabilities` first. It reports the exact protocol version, available
operations, closed vocabularies and retrieval budgets supported by the build.
It works even before a project has initialized a knowledge base. Do not
duplicate the vocabularies in Repository 2.

## Coding agents (Claude Code, Codex, Cursor)

People's own coding agents are clients of the same bridge, through `athenaeum_integrations`: an MCP server
(`wiki_*` tools) and, for Claude Code, hooks that recall knowledge for each prompt and tidy up when a turn
ends. Athenaeum installs and wires them with `athenaeum setup` (see [installing.md](installing.md)); the
configuration it writes into each agent's settings is owned by `athenaeum_integrations`. Nothing about the
contract changes: they call `context`, `search`, `open`, and, with the person's grant, `create` (which only
proposes). They are never given `accept`, `reject` or sync by default, and a hook that cannot reach the bridge
stays silent so the agent carries on as an ordinary coding agent.

## Operations

| Method | Purpose | Mutates knowledge |
| --- | --- | --- |
| `capabilities` | Protocol negotiation, vocabularies and budgets | No |
| `search` | Find candidate nodes by lexical lookup | No |
| `open` | Full node, relationships, evidence, history and utility | No |
| `navigate` | Budgeted graph walk from a node | No |
| `related` | One-hop relationships; proposals are opt-in | No |
| `evidence` | Evidence attached to a node | No |
| `history` | Archived node revisions | No |
| `context` | Structured brief plus trust-wrapped prompt text | Retrieval journal only |
| `create` | Store a model-attributed, unverified node | Yes |
| `update` | Version a node; journal the model/session that edited it | Yes |
| `link` | Store a model-attributed proposed relationship | Yes |
| `attach_evidence` | Attach source evidence to a node | Yes |
| `record_use` | Report which retrieved nodes actually helped | Utility journal only |
| `status` | Workspace and index state; answers with no vault present | No |
| `review` | Findings that need a decision, as one bounded list | No |
| `sources` | Files under `raw/sources/` with hash, coverage and staleness | No |
| `index` | Index the vault, the repository, or both | Rebuilds derived state |
| `path` | How one node connects to another; `found:false` is an answer | No |

Node references accept either an ID or an exact title. `context.seeds` may be
omitted; the library will search for seeds and then navigate. All caller
budgets are capped by advertised limits.

Read requests put their arguments beside `method`:

```json
{ "method": "search", "query": "OAuth callback", "limit": 20 }
{ "method": "open", "node": "AuthService" }
{ "method": "navigate", "node": "AuthService", "depth": 2, "maxNodes": 25 }
{ "method": "related", "node": "AuthService", "includeProposed": true }
{ "method": "evidence", "node": "AuthService" }
{ "method": "history", "node": "AuthService" }
{
  "method": "context",
  "task": "fix the OAuth callback",
  "seeds": ["OAuth"],
  "depth": 2,
  "maxNodes": 25
}
```

Curator mutations require a caller. `session` is the opaque Repository 2
session ID, not a Wiki-owned conversation:

```json
{
  "method": "create",
  "input": {
    "type": "observation",
    "title": "OAuthCallback validates state first",
    "summary": "State is checked before session creation.",
    "scope": "project"
  },
  "caller": { "model": "curator-model", "session": "sess_123" }
}
```

`update.input` accepts `node` plus any of `title`, `summary`, `content` and
`reason`. `link.input` accepts `from`, `type`, `to`, and optional `note` and
`confidence`. `attach_evidence.input` accepts `node`, `kind`, and optional
`anchor`, `excerpt` and `note`; it also requires `caller`. After a task, the
runtime can report utility with:

```json
{
  "method": "record_use",
  "nodes": ["Decision_042", "OAuthCallback"],
  "session": "sess_123"
}
```

## Product state: `status`

A client that cannot tell "no vault here" from "an empty vault" from "a vault whose index is
behind" will report a confident lie. `status` answers all three, and unlike every other method
except `capabilities` it succeeds with no vault at all — absence is a state, not a transport
error:

```json
{ "method": "status" }
```

```json
{
  "method": "status",
  "status": {
    "workspacePath": "/home/x/project",
    "initialized": true,
    "hasVault": true,
    "vaultPages": 42,
    "nodes": 310,
    "edges": 288,
    "proposals": 3,
    "findings": 7,
    "indexedAt": "2026-10-09T09:12:44Z",
    "indexStale": false,
    "protocolVersion": 1,
    "engineVersion": "0.1.0"
  }
}
```

* `initialized` — `.athenaeum/` exists. `false` means every other field is a zero and
  `indexedAt` is `null`; a client shows the "no knowledge base here yet" state rather than an
  error, and says how to create one (`athenaeum init`).
* `hasVault` — the project holds a vault (`wiki/`). A project can be indexed for code and have
  no vault at all, which is a legitimate shape.
* `vaultPages`, `nodes`, `edges`, `proposals` — the vault, the derived index and the queue a
  person still has to decide about. `proposals` counts unreviewed proposals; `findings` counts
  the same findings `review` returns.
* `indexStale` — an index **exists** and the sources it describes have moved on since. A project
  that has never been indexed reports `indexedAt: null` and is rendered as an empty (or absent)
  knowledge base, never as a stale one; the two states mean different things to a person, and a
  client that conflates them will offer to re-index a directory that has nothing in it.
  Staleness is **reported, never silently repaired** by this method: `status` does not write.
  Reads (`search`, `open`, `navigate`, `related`, `evidence`, `history`, `context`, `proposals`)
  do reconcile first via the `vault_sync` feature, so a client that wants a question answered
  from current markdown asks a read; a client that wants to know whether the index is behind
  asks `status`. `indexedAt` is the newest of the vault and repository index times; `null` means
  nothing has been indexed yet.
* `engineVersion` — the build that answered. Compare it with `protocolVersion` when reporting a
  problem: a version skew is a deployment fact, not a client bug.

A stale index is not an error. `status.indexStale`, `sources[].stale` and the
`incomplete_scan` finding are the three places it surfaces; none of them is an error code.

## Product state: `review`

```json
{ "method": "review", "limit": 20 }
```

```json
{
  "method": "review",
  "findings": [
    {
      "id": "finding_contradiction_1",
      "kind": "contradiction",
      "severity": "blocking",
      "title": "Validate state first",
      "detail": "disagrees with \"Create the session first\", and nobody has decided between them",
      "nodes": ["node_9f2c", "node_4a11"],
      "evidence": ["ev_3a6fc937115e432e"],
      "sourcePath": null,
      "confidence": 1.0,
      "blocking": true
    }
  ]
}
```

`id` is stable for as long as the finding is: the same base reports the same findings in the
same order, so a caller can diff two `review` calls and mean something by it. `nodes` holds node
ids, `evidence` holds evidence ids, and `sourcePath` names the file a stale anchor was taken
from, when the finding is about one.

`kind` is a closed vocabulary:

| Kind | Meaning | Severity |
| --- | --- | --- |
| `contradiction` | Two live claims disagree and nobody has decided | blocking |
| `dispute` | Several live claims describe one subject; one governs | blocking |
| `stale_anchor` | Its source moved, changed or could not be recovered | blocking when lost, warning when moved |
| `unsupported_rule` | A rule or decision resting on no evidence | warning |
| `unreviewed_proposal` | A model proposal, or a proposed relationship, awaiting a decision | info |
| `duplicate` | Two nodes with the same title | warning |
| `near_duplicate` | Two titles that differ only in punctuation or case | warning |
| `superseded` | Kept for history, no longer offered | warning, or info when it merely supersedes without saying so |
| `incomplete_scan` | A scan budget ended the walk before the tree did | warning |
| `orphan` | Live, but no traversable relationship reaches it | warning |

Severity is the engine's judgement about what the finding means, and `blocking` is exactly
`severity == "blocking"`.

`blocking: true` means the finding could hand a model something incorrect; it is exactly the
findings whose `severity` is `blocking`. A client may page the list with `limit`; there is no
offset, because findings are regenerated deterministically, not stored.

## `sources`

Every file the index has read — sources under `raw/sources/` and repository files — with its
hash, how much of the graph points at it, and whether it has moved since:

```json
{ "method": "sources", "limit": 50 }
```

```json
{
  "method": "sources",
  "sources": [
    {
      "scope": "vault",
      "path": "raw/sources/design-notes.md",
      "bytes": 78,
      "sha256": "9f2c…",
      "scanned": true,
      "nodes": 2,
      "anchors": 2,
      "indexedAt": "2026-10-09T09:12:44Z",
      "stale": false
    },
    {
      "scope": "repository",
      "path": "src/auth/oauth.ts",
      "bytes": 2048,
      "sha256": "c0ff…",
      "scanned": true,
      "nodes": 1,
      "anchors": 1,
      "indexedAt": "2026-10-09T09:12:44Z",
      "stale": false
    }
  ]
}
```

* `scope` — which pass produced the record, `"vault"` or `"repository"`. A path both passes read
  appears **once per scope**, because the two records describe different things: a file of the
  code repository, and source material the wiki cites. The pair is the identity, not the path
  alone.
* `path` is project-relative with forward slashes, so the same source names the same file on
  every platform.
* `sha256` is the hash recorded when the file was read — empty for a file that was deliberately
  not read, and never silently absent for one that was.
* `scanned` — the file was inside the last scan's coverage. `false` means a budget or a filter
  left it out — the same paths appear in `index.skipped` — which is why it is reported rather
  than omitted; `indexedAt` is then `null`.
* `nodes` / `anchors` — how many knowledge nodes rest on it, and how many evidence anchors point
  into it. A source nothing cites is material, not knowledge.
* `stale` — the file is gone, or has changed since it was read. A `stat` comparison, not a
  re-read: this is asked on a poll.

## `index`

```json
{ "method": "index", "scope": "all" }
```

```json
{
  "method": "index",
  "index": {
    "vaultPages": 42,
    "repositoryFiles": 137,
    "symbols": 604,
    "edges": 288,
    "durationMs": 412,
    "incomplete": false,
    "skipped": []
  }
}
```

* `scope` is `"vault"`, `"repository"` or `"all"`. `paths` is an optional list of
  repository-relative paths that narrows a **repository** pass; a narrowed pass never marks
  anything stale, because it did not see the whole tree. Vault scope with `paths` is
  `invalid_request` rather than a silent partial reconcile.
* `skipped` names what the pass deliberately did not index — files outside the requested scope
  that a caller might have expected, or paths a filter excluded. It is not a warning list.
* `incomplete: true` means a budget or an unreadable file stopped the walk short. Nothing was
  deleted on that basis: a truncated scan is never evidence that knowledge disappeared.

## `path`

```json
{ "method": "path", "from": "Authentication", "to": "Validate state first", "depth": 6 }
```

```json
{
  "method": "path",
  "path": {
    "from": "Authentication",
    "to": "Validate state first",
    "found": true,
    "steps": [{ "from": "Authentication", "edgeType": "references" }],
    "nodes": []
  }
}
```

`found: false` is an **ok** envelope with an empty `steps`: the two nodes exist and nothing
connects them within the budget. It is the answer to "why is this relevant to that", and a
client shows it rather than treating it as a failure. Node references follow every other method:
an id or an exact title. Each step carries the relationship and, when the edge has one, its note
and confidence — the reason the relationship exists, verbatim, so a reader can judge it.

## Pages: which file a node is

The vault is the source of truth, and an agent changes it with its ordinary file tools. So a node
that *is* a page of the vault says which file, in `open` and in a brief:

```json
{
  "method": "open",
  "detail": {
    "node": { "id": "vault_page_…", "title": "Validate state first", "…": "…" },
    "page": {
      "path": "wiki/decisions/validate-state.md",
      "absolutePath": "/home/x/project/wiki/decisions/validate-state.md",
      "contentHash": "b8163dbf…"
    }
  }
}
```

A brief node carries the same `page`, and the brief's prompt states it as a plain line under the
node (`Page: /home/x/project/wiki/decisions/validate-state.md`), so a task can open with the
location of what it should change and no round trip per node.

* `path` is the page's identity: vault-relative, forward slashes, the same on every platform.
* `absolutePath` is what a file tool can open on this host. The library resolves it, because only
  it knows where the project is, and the runtime must not join paths against a root it guessed. It
  is omitted when the library cannot say where the project is. It is returned even when the vault
  is outside what the runtime's sandbox can reach: the sandbox refusing the read is a clearer
  failure than a pointer that silently vanished.
* `contentHash` is the SHA-256 of the page as the index last read it. A page edited since then
  hashes differently on disk, which is how a reader holding a node can tell it is behind the file.
* The field is **absent** — not `null` — on every node that is not a page: a raw source, a node a
  model proposed, a symbol from the repository scan. Consumers must not teach a model that every
  node has a file.

A pointer is an addition to the node and never a substitute for it. The page holds the prose; the
relationships, evidence, history and utility are in the index and nowhere in the markdown, so
reading the file in place of opening the node loses exactly what the knowledge base is for.

Nothing is stored for this. The indexer already attaches one `Documentation` record to each page,
anchored at its file and carrying the hash it read, and the pointer is read back off that record.

**Feature negotiation.** `capabilities.features` lists optional behaviours beyond the operations.
`page_pointers` means the above. A library that predates the list reports none, so an absent
`features` and an absent `page` mean "an older library, do not assume", while a present feature and
an absent `page` mean "this node is not a page".

**A page is changed in its file, not through `update`.** The page's text is the markdown, and the
index follows the vault, so an `update` made to the node would be discarded by the next reconcile.
The library refuses it with `invalid_request` and a message that names the file, which is clearer
than accepting an edit that silently vanishes. `link` and `attach_evidence` are unaffected: a
relationship or evidence a model proposes is not derived from the markdown and survives every
reconcile.

**Staying current (`vault_sync`).** An agent changes the vault with file tools, and a headless
session has nothing watching it the way the desktop app does. Left alone, a question asked just
after an edit would be answered from the index as it was before the edit, with nothing to say so. So
before the bridge answers a *read* — search, open, navigate, related, evidence, history, context —
it compares a fingerprint of the vault's files (path, length and modification time; no file is
read) with the one recorded when the vault was last indexed, and reconciles only when they differ.
The same pass that picks up an edit picks up a page the agent created or deleted.

* An unchanged vault costs one directory walk: measured at about +4 ms at 1,000 pages and +55 ms at
  10,000, against a read that takes 21 ms and 113 ms. A reconcile after a change is a full re-read
  of the vault, 0.3 s and 3 s at those sizes, paid once rather than on every call.
* It **keeps an index that exists; it never builds the first one.** A first index of a large vault
  outlasts what a caller waiting on an answer should be made to wait, so it stays explicit:
  `athenaeum index-vault`, or opening the project in the app. Until then a read simply finds no
  vault knowledge.
* A mutation does not trigger it, and a failure to refresh never fails the read: the answer comes
  from the last index, exactly what it would have been without the step, and the reason is written
  to stderr.
* The fingerprint is a heuristic. An edit that leaves both length and modification time unchanged
  is invisible to it; nanosecond timestamps make that a non-event on the filesystems the app ships
  on, and the next explicit index repairs a miss.

**Renaming.** A page's node id is a hash of its path. Editing a page keeps its node; moving or
renaming it is a different page to the indexer, which removes the departed node and creates a new
one, and the utility history and any model-proposed edges go with the old id. That is deliberate
for now: carrying identity across a rename by content hash would make the index depend on the
history of the vault, which breaks the property that the index is a function of it (delete
`knowledge.db` and you lose nothing but time). Until that trade is revisited, a runtime tells its
model to edit pages and not move them.

## The CLI, as the human's scripting surface

`athenaeum` is the same engine without a window, and the transport every non-Rust client uses.
The commands a person scripts map one-to-one onto the methods above:

| Command | Method it answers from |
| --- | --- |
| `athenaeum init [--json]`, `repo init`, `repo info` | workspace and repository identity |
| `athenaeum status [--json]` | `status` |
| `athenaeum index [--full] [--json]`, `athenaeum index --scope vault\|repository\|all [--path P] [--json]` | `index` |
| `athenaeum index-vault` | `index` with vault scope |
| `athenaeum sources [--limit N] [--json]` | `sources` |
| `athenaeum check [--strict] [--json]`, `athenaeum review [--limit N] [--strict] [--json]` | `review` |
| `athenaeum search`, `show`, `list`, `navigate`, `path`, `brief`, `history`, `journal` | the read methods |
| `athenaeum proposals`, `accept`, `reject`, `draft-source` | the proposal lifecycle |
| `athenaeum lint`, `relocate`, `evidence`, `verify`, `supersede`, `contradict`, `link` | maintenance |
| `athenaeum export`, `import`, `trust`, `vault materialize`, `import-legacy-core` | portability |
| `athenaeum library …`, `athenaeum sync …` | Library API v1, never the bridge |

Plain `index` and `index --scope` differ on purpose. Plain `index` prints the detailed repository
report (files, symbols, imports, commits, warnings) and takes the budgets
(`--max-files`, `--max-file-bytes`, `--max-total-bytes`, `--max-commits`); `--full` reads every
file again instead of reusing the ones whose size and modification time are unchanged, which is
what re-indexing a large tree cheaply depends on. `index --scope` prints the one unified summary
and takes none of those budgets — the engine's own apply — so the two flag sets conflict rather
than one silently winning.

A relationship carries its reason. `athenaeum link … --note "…" --confidence 0..1` records one,
and `show`, `navigate` and `path` print it beside the relationship — the note verbatim, the
confidence only when the edge has one — so a reader can judge the edge and not just the arrow. A
confidence outside `0..=1` is refused at the boundary rather than stored and re-rendered as
something a reader would take at face value.

Every command honours `--json`, and the value printed as JSON is the same value the human
rendering is made from — two separately built representations drift. Failures are JSON too when
`--json` was asked for: `{"error":{"code":"…","message":"…"}}` on stdout, the message also on
stderr, and a non-zero exit.

**Exit codes.** `0` for an answer, `1` for a failure or a state the command exists to detect.
`check` and `review` exit `1` only when a blocking finding is present (`--strict` makes every
finding count), because being untidy is not a build failure. `status` exits `0` with
`initialized: false` when there is no vault: absence is the answer. `path` exits `0` whether or
not a path was found, matching `found: false` on the bridge. Every other command exits `1` with
`workspace_not_found` on a directory that has not been initialised. A failure under `--json`
prints `{"error":{"code":"…","message":"…"}}` on stdout using the same code the bridge would
return for the same condition — `knowledge_error` for a store that cannot be read,
`invalid_request` for a value the library refuses — so a script branches on one vocabulary.

**A stale index is never silently trusted.** The CLI's read commands reconcile the vault index
before they answer, exactly as the bridge does for a read, and print what changed on stderr so
`--json` output on stdout stays clean. `status` and `review` do not write; they report
`indexStale` and `incomplete_scan` instead.

**The Library is a place to publish, and nothing here needs it.** `library` and `sync` are the
only commands that touch a network. When the server cannot be reached they report an explicit
`offline` error, write nothing, and leave local work exactly as it was; `sync status` says
`offline` without failing. There are no hosted features and no background sync.

## Runtime-side adapter

The runtime defines the interface in its own terms and translates it to
protocol v1. That interface exists: the
[`athenaeum_agent`](https://github.com/Hanoryx-North/athenaeum_agent) bundle
declares a knowledge capability on the DeepSeek Harness context, with a
provider that speaks this protocol over the CLI bridge:

```text
ctx.wiki                       the capability, with provider selection
  capabilities()
  execute(request)             any protocol v1 operation
  context(task, budgets)       nothing when a brief would be empty
  recordUse(nodes, session)

athenaeum provider             one-shot bridge, negotiated once
```

Read methods reach the model as `wiki_*` tools; the mutation methods are
registered only for curator-enabled deployments. A runtime with no provider
registered is the equivalent of the null implementation, and stays a
legitimate coding agent.

Do not make this crate implement the runtime's interface directly. That would
reverse the dependency and couple the standalone runtime to this storage
implementation.

## Trust, permissions and provenance

`context` returns both a structured `brief` and a `prompt`. The prompt is the
only pre-rendered text that should be injected into model context. It states
that retrieved knowledge is reference material and cannot override system
instructions, grant permissions, or authorize tools.

The bridge is a local knowledge API, not an authorization system. Repository
2 must run mutation calls through its own permission/curator policy. On this
side, mutations preserve the knowledge trust model:

- `create` requires the model name and optionally accepts the runtime session
  ID; the node starts with `unverified_observation` authority.
- `link` requires the same caller identity; the edge starts `proposed` and is
  not traversed until promoted.
- `update` archives the previous revision and attributes the editing action in
  the journal without replacing the claim's original provenance.
- every mutation and retrieval/use event is journaled by the store.

Repository 2 should retain its own conversation and tool history. Only its
opaque session ID crosses this boundary for provenance; agent messages do not
become Wiki knowledge automatically.

## Compatibility rules

- `protocolVersion` changes only for breaking wire or semantic changes.
- Additive response fields do not require a version bump; consumers must
  ignore unknown fields.
- Unknown methods, enums, **fields** and malformed inputs fail loudly. A field the engine does
  not know is refused with `invalid_request` and a message that names it — the same treatment an
  unknown method gets, on purpose. The compatibility rule this encodes:
  * The engine is updated **first** to accept a new optional field. Existing clients that do not
    send it are unaffected, which is the additive direction v1 guarantees.
  * A newer client sending a field an older engine does not know gets `invalid_request` naming
    the field. That is the protocol-mismatch signal — an adapter maps it to its own
    "incompatible" or "protocol_mismatch" error — and it is the honest outcome, because the older
    engine could not honour the field anyway. Silently ignoring it would answer a different
    question than the one that was asked.
  * So a client never has to guess: `capabilities.operations` and `capabilities.features` say
    what the build supports before anything is sent, and a request built beyond that is refused
    rather than half-answered.
- A client requesting a newer protocol receives `unsupported_protocol`; the
  message names the version requested and the version supported. The
  database is never opened through an assumed schema.
- Stable error codes in v1 are `invalid_request`, `not_found`,
  `incompatible_store`, `knowledge_error`, `request_too_large`,
  `unsupported_protocol` and `workspace_not_found`. The last tells the
  adapter to behave like `NoWikiProvider` for that workspace, and is not
  returned by `capabilities` or `status`.
- A stale index is not an error and has no code of its own: read
  `status.indexStale`, `sources[].stale` and the `incomplete_scan` finding.
  Inventing an error for it would make a client treat "the markdown moved"
  as "the engine is broken".
- `offline` is a CLI-side code from the Library client (`library`, `sync`)
  when a server cannot be reached. It is never a bridge response: the bridge
  makes no network calls.
- The CLI bridge is the cross-language compatibility harness. Its integration
  tests must stay offline and run on Windows, macOS and Linux.

Duplicate and conservative near-duplicate create errors add a
`details.existingNodes` array. A curator should use one of those IDs for
`attach_evidence` or `update`; it must not parse the human error message to
recover an identity.

## Language-neutral fixtures

Protocol v1 request and response fixtures are published at
`fixtures/wiki-protocol/v1/`. The request catalog covers every operation
advertised by `capabilities`; representative successful and failed responses
pin envelope shape, casing, vocabularies, limits and the retrieval trust
wrapper. The Rust suite round-trips every fixture through the production types
and compares the capabilities fixture with the actual service response.

The runtime runs the same files through its adapter tests: `athenaeum_agent`
vendors this directory with the commit it was taken from, compares every
request it builds against the published envelopes, and replays every response
through its production client. The fixtures are examples of the wire contract,
not permission grants and not an interface definition for the runtime.
