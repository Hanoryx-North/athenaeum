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
- Unknown methods, enums and malformed inputs fail loudly.
- A client requesting a newer protocol receives `unsupported_protocol`; the
  database is never opened through an assumed schema.
- Stable error codes in v1 are `invalid_request`, `not_found`,
  `incompatible_store`, `knowledge_error`, `request_too_large`,
  `unsupported_protocol` and `workspace_not_found`. The last tells the
  adapter to behave like `NoWikiProvider` for that workspace.
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
