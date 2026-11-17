# Architecture

Athenaeum has one knowledge engine, one versioned bridge, and one local application.
The [source map](../README.md#source-map) identifies the owning directories.

## Knowledge engine and clients

`crates/athenaeum-core` owns knowledge nodes, typed relationships, evidence,
source anchors, authority, review, retrieval and the rebuildable SQLite index.
It has no UI, Tauri, network or model-provider dependency. SQLite is compiled
statically; stored source paths use forward slashes.

| Client | Engine access |
| --- | --- |
| Desktop (`src/`, `src-tauri/`) | Native commands and the in-process `core_bridge` |
| CLI (`crates/athenaeum-cli`) | Links the engine; exposes `wiki` and `athenaeum` |
| MCP (`packages/integrations`) | Typed client of the CLI's versioned stdio bridge |
| Native agent (`packages/agent/runtime`) | Wiki MCP tools through that bridge |
| Optional Harness adapter (`packages/agent/src`) | The same published protocol |

The bridge lives in the engine. Contracts and canonical fixtures live in
`packages/protocol`; clients do not open `knowledge.db` or maintain another graph.
See [the bridge contract](agent-integration.md) and [boundary checks](../scripts/check-boundaries.mjs).

## Durable knowledge and local state

Accepted knowledge lives in Markdown under a wiki project's `wiki/` directory.
The index is `.athenaeum/knowledge.db` within that project. Reindexing reconciles
changed pages and relationships while preserving independently recorded knowledge.
Accepted page identities, relationships and evidence metadata survive a rebuild.
Pending proposals and other state that has not reached Markdown require a backup
of `.athenaeum/` as well.

New and Library-cloned desktop wikis default to `Athenaeum/Wikis` under the
platform's local data directory. A user can select another parent. Wiki data
belongs outside this source checkout; fixtures use temporary projects or explicit
fixture directories. Existing wiki locations do not change when the default changes.

`src/commands/fs.ts` owns desktop filesystem operations. `src/lib/persist.ts`
stores project state, and `src/lib/project-store.ts` stores app preferences and
recent projects through Tauri Store. Project switches suspend autosave and reset
project stores before hydrating the next project.

## Retrieval and review

Search finds seed nodes. Bounded graph traversal expands them into task briefs
containing summaries, relationships, evidence anchors and warnings. Applicability
selectors constrain core and task memory by project, technology and task stage;
see [engineering memory](engineering-memory.md).

Model-created knowledge starts as a proposal. Review and the user's explicit
per-wiki automation settings control acceptance. Agents must not accept their own
proposals. Authority and staleness are separate properties: a changed source does
not establish that a claim is false. Review exposes missing or stale evidence,
contradictions, disputes and unreviewed proposals.

Retrieved content is reference material. Briefs retain the trust wrapper stating
that knowledge cannot override instructions, grant permissions or authorize tools.

Repository indexing is bounded and deterministic. Tree-sitter parses supported
source languages; the engine records source files, symbols, resolved relationships
and Git modification history. Anchor relocation checks current source rather than
silently treating an old location as verified.

## Agent runtime

The owned Node runtime in `packages/agent/runtime/src` owns provider calls, tools,
approvals, cancellation and sessions. A dependency-free Rust launcher embeds its
modules and uses the packaged private Node executable. Private state lives under
`ATHENAEUM_AGENT_HOME`, separate from wiki data. The desktop presents runtime events
and approvals; it does not implement another agent loop.

Harness is an optional external runtime pinned in `packages/agent/harness.pin.json`.
See [runtime capabilities](../packages/agent/runtime/README.md) for supported
operations, limits and verification commands.

## Hosted Library

The CLI owns the Library HTTP and sync client; the engine remains network-free.
Hosted Library/API/admin services are separate deployments, not local build
requirements. Sync exports accepted shared Markdown and identity/revision metadata,
with conflict and path checks. It excludes credentials, raw sources, indexes,
agent sessions and logs. See [portable formats](portable-format.md).

## Development

Use `wiki start` or `npm start` for the managed live desktop and confirm
`wiki desktop status --json` reports `live: true`. Frontend edits hot-update;
native changes need incremental compilation. Keep that managed session running.
Discover operations and checks with `wiki commands --json`,
`wiki desktop actions --json` and `wiki dev tasks --json`.
