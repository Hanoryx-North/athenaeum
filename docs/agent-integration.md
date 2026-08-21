# Agent Runtime Integration

Athenaeum is **Repository 1: the Wiki Library**. The coding agent is a separate
repository and must remain independently usable when this library is absent.
This repository does not own the agent loop, coding tools, shell, Git,
streaming, context compaction, sessions, or the agent permission system.

The integration is intentionally shaped like this:

```text
LLMWiki Agent Runtime (Repository 2)
  Agent loop · providers · tools · permissions · sessions
                    │
                    │ WikiProvider adapter
                    │ protocol v1 JSON
                    ▼
Athenaeum / Wiki Library (Repository 1)
  context · graph · curation · evidence · history · knowledge.db
```

The runtime knows the versioned Wiki protocol. It does not know that this
implementation uses Rust, SQLite, FTS5, or content-addressed blobs.

## Repository ownership

| Repository 1 — this repository | Repository 2 — agent runtime |
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

`crates/athenaeum-providers` predates this repository split. It remains as
bootstrap compatibility code for the current shell, but it is **not** the
future runtime boundary and no agent loop should be built around it here.
Provider ownership and further provider development belong to Repository 2.

## Integration contract

The typed implementation is in `athenaeum_core::wiki`. A consumer can use it
in-process through `WikiService`, or use the language-neutral CLI bridge:

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

## Runtime-side adapter

Repository 2 should define its own small interface in runtime terms. The
minimum useful shape is:

```text
WikiProvider
  capabilities()
  get_context(task, workspace)
  execute(request)

NoWikiProvider
  get_context(...) -> no additional context
```

The Athenaeum adapter translates those calls to protocol v1. `NoWikiProvider`
keeps the coding agent independently usable. The primary agent can register
the read methods as `wiki.*` tools; curator-controlled flows can additionally
register mutation methods.

Do not make this crate implement Repository 2's interface directly. That
would reverse the dependency and couple the standalone runtime to this
storage implementation.

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
