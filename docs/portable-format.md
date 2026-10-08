# Portable format and sync protocol

> **Status: Stage 2 contract, decided 2026-10-09.**
> **Built and tested:** declared page ids and their collision rules (section 4, "Identity and
> acceptance", up to "Proposal, acceptance, rejection"), in `vault/page.rs` and
> `vault/index.rs`. **Not built:** everything else, including accept and reject, `materialize`,
> the manifest, the authority cap, export and import, MCP, and sync.
> Where this document and the code disagree, the code is what runs today and this document is
> what is being built; the disagreement is a bug in one of them and gets fixed, not left.

One knowledge repository has to be usable by the desktop app, the `athenaeum` CLI, the agent,
MCP clients, a Library server and a second computer. This document fixes what that
repository *is*, what leaves a machine when it syncs, and the wire protocol that carries it.
It extends the vault that exists. It does not introduce a second format.

## 1. Two findings that shaped it

Both were reproduced with the real `athenaeum` binary, not read from documentation.

**Knowledge that is not a page is not portable.** Nodes created with `athenaeum add` and by
the bridge's `create` live only in `knowledge.db`. Index a two-page vault, add two nodes
outside it (one with `--stated`, a decision typed by the user), delete `knowledge.db` and run
`index-vault`: both nodes are gone, and nothing in the vault files mentioned them.
[`architecture.md`](architecture.md#storage) describes this correctly ("canonical data is
whatever was not derived from the vault"). `AGENTS.md` and the vault documents state the
opposite test, "delete `knowledge.db` and you lose nothing but time", which holds only for
what the vault implies. Syncing the vault alone would silently drop the agent's accepted
memory.

**The vault trusts its own frontmatter.** `authority_for` returns a page's declared
`authority:` as written. A page containing `authority: user_instruction` indexes as a
`user_instruction` node, with `deterministic` provenance and no evidence check, where
`athenaeum verify` would have asked for evidence. That is reasonable for a vault only its
owner writes. It is unsafe the moment a page can arrive from a pull, a fork or an import.

## 2. Principles

1. **The vault is the source of truth.** `knowledge.db`, search indexes, graph indexes,
   embeddings and caches are derived and rebuildable. Anything else required to reconstruct
   persistent knowledge is text in the vault.
2. **Accepted knowledge is a page.** A proposal is not durable until it is accepted
   (section 4). This is the memory rule: conversation content never becomes permanent
   knowledge by itself.
3. **Arrival is not authority.** A claim of authority in text that came from elsewhere is a
   claim, not a fact (section 5).
4. **Unknown data survives.** No migration, rewrite or sync step drops frontmatter keys,
   files or manifest fields it does not understand.
5. **No data loss on divergence.** Two edits to one file never resolve by one overwriting the
   other (section 7).
6. **The cloud stores text.** Library never parses a page into nodes, edges or embeddings.

## 3. Repository layout

```text
my-knowledge/
├── athenaeum.json        manifest: identity and format version        (new, optional)
├── README.md
├── purpose.md  schema.md  index.md  log.md
├── wiki/**/*.md          pages: the knowledge itself                  (exists)
├── raw/sources/**        source material                              (exists)
└── .athenaeum/           local only: knowledge.db, blobs/, cache/, sync/   (exists)
```

`athenaeum.json`:

```json
{
  "formatVersion": 1,
  "repositoryId": "repo_3f9c2b7a-1d4e-4c8a-9b55-6a0e7d2c1f44",
  "name": "payments-service",
  "createdAt": "2026-10-09T01:00:00Z"
}
```

- `repositoryId` never changes, whatever the repository is renamed to. It is generated once.
- A vault without a manifest is a legacy vault. It opens and works; `athenaeum repo init`
  adds the manifest. A missing manifest is never an error.
- Fields the reader does not know are preserved on rewrite. A `formatVersion` higher than the
  reader supports is refused, not downgraded.

### What syncs

The **sync set** is every regular file under the repository root that is not under a
directory whose name starts with `.` and that:

- has an extension in `.md .txt .json .yaml .yml .toml`;
- is valid UTF-8, contains no NUL byte, and is not a symlink;
- is within the server's advertised per-file limit (default 1 MiB) and the repository
  limit (default 50 MiB);
- has a normalised path: `/` separators, no `.` or `..` segment, no leading `/`, no
  drive letter, no control characters, at most 255 bytes per segment and 1,024 in total.

Everything else stays local and is reported as **skipped**, with the reason, in the sync
result. In particular: `.athenaeum/` (databases, embeddings, caches, agent state), binary
files in `raw/sources/` (a citation to one survives as text in the page that cites it), and
anything executable. A skipped file is never an error.

## 4. Identity and acceptance

Every knowledge item has one stable identifier. A page declares it:

```yaml
---
id: Decision_042
type: decision
title: Retry budget is three
authority: unverified_observation
origin: agent-proposal
accepted: 2026-10-09T01:12:00Z
sources: ["adr-0042.md"]
related:
  - "[[retry-policy]]"
---
```

- `id:` is optional. Without it a page keeps today's id, a hash of its vault-relative path, so
  no existing vault changes and no existing id moves.
- With it, the node id is that value. It survives moving or renaming the file, which closes
  the gap `vault-seam.md` left open on purpose: a page that declares its identity can be
  moved without orphaning its edges, evidence and utility history.
- Grammar: `[A-Za-z0-9][A-Za-z0-9._-]{0,127}`, and not beginning with `vault_`, which is
  reserved for path-derived ids. An invalid `id:` is ignored and the page falls back to its
  path id; `lint` reports it.
- Two pages declaring one id: the page whose path sorts first keeps it, the rest fall back
  to path ids, and `lint` reports `duplicate_id`. The rule depends only on vault contents,
  so the index stays a function of the vault.

### Proposal, acceptance, rejection

| Operation | Effect | Touches the vault |
| --- | --- | --- |
| Propose (`create`, `update`, `link`) | A node in `knowledge.db`, model-attributed, unverified. As today. | No |
| **Accept** | Writes the node as a page with `id:` set to its node id, so its edges, evidence and utility history carry over. Authority is unchanged: accepting is not verifying. | Yes |
| **Reject** | Marks the proposal rejected so it is not offered again. | No |
| `verify` | As today. Raises authority only when the evidence supports it. | Edits `authority:` in the page |

A node stated by the user (`provenance: user`) is accepted by being stated, and is written as
a page when it is created. The bridge gains `accept` and `reject` requests. Both are writes,
require a caller, and are refused without the write permission (section 8).

**Migration of existing databases.** `athenaeum vault materialize` writes pages for existing
nodes that are user-stated or verified and are not already vault-owned. It defaults to a
dry run, never deletes a database row, never overwrites an existing file (a name collision
gets a numeric suffix), and reports every node it left alone. Model proposals are not
materialised; they stay proposals.

**Pages are rewritten surgically.** Accept, verify and migration edit the keys they own and
leave the rest of the frontmatter and body byte-for-byte as they were.

## 5. Arrival is not authority

The sync client records, per path, the hash of what it last wrote from the remote and where
that content came from, in `.athenaeum/sync/<repositoryId>.json` (local; never synced):

```json
{ "wiki/auth.md": { "sha256": "…", "origin": "pulled" } }
```

While a page's current hash equals a hash recorded as `pulled`, the indexer caps its
authority at `imported_knowledge`, whatever its frontmatter declares. Any local edit, and any
`verify` or accept, changes the hash and lifts the cap: that is the moment a person took
responsibility for the text. Content a device pushed itself is recorded as `pushed` and is not
capped on that device. A freshly cloned repository is therefore entirely `imported_knowledge`
until someone reviews it.

The cap is in addition to, not instead of, the trust wrapper: retrieved knowledge is
reference material, never instruction, whatever its authority.

## 6. Revisions

Library keeps linear **snapshot revisions**; there is no branch model in v1.

- A **manifest** is the sorted list of `{ "path", "sha256", "size" }` for the sync set,
  serialised as canonical JSON (keys sorted, no insignificant whitespace, UTF-8).
- `treeHash` is the SHA-256 of the canonical manifest.
- A **revision** is `{ "parent", "treeHash", "author", "message", "createdAt", "mergedFrom"? }`.
  Its id is `rev_` followed by the SHA-256 of its canonical serialisation, as 64 hex digits.
  The server assigns it. A client can recompute `treeHash` from what it holds and so verify
  that it received exactly the tree the revision names.
- `mergedFrom` records the remote revision a client merged before pushing. It is history for
  display; the chain stays linear.
- File content is stored by `sha256`, once per repository, and reconstructed on demand. Diffs
  are computed from stored content when asked for, not stored.

This model is chosen over Git because the sync unit is a small text tree, the server never
needs packfiles or refs, and compare-and-swap on one head is the whole concurrency story.

## 7. Library API and sync protocol v1

All routes live under `/api/v1`. Resources are addressed by the immutable repository id, so a
rename never breaks a synced device. `GET /api/v1/users/{username}/repositories/{name}`
resolves a human name to the id.

| Method and path | Purpose | Scope |
| --- | --- | --- |
| `GET /api/v1` | `{ service, apiVersion: 1, syncProtocol: 1, limits }` | none |
| `POST /api/v1/auth/device`, `POST /api/v1/auth/device/token` | Device authorisation flow (below) | none |
| `GET /api/v1/repositories/{id}` | Metadata, visibility, `head` | `repo:read` or public |
| `GET /api/v1/repositories/{id}/revisions/{rev}` | Revision and its manifest | `repo:read` or public |
| `GET /api/v1/repositories/{id}/blobs/{sha256}` | File content, only if the repository contains it | `repo:read` or public |
| `GET /api/v1/repositories/{id}/revisions` | History, newest first | `repo:read` or public |
| `GET /api/v1/repositories/{id}/compare?from=&to=` | Per-file text diff | `repo:read` or public |
| `POST /api/v1/repositories/{id}/push` | Create the next revision | `repo:write` |

**Push.** One request, one transaction:

```json
{
  "base": "rev_…",
  "message": "Accept 3 proposals",
  "changes": [
    { "path": "wiki/retry-policy.md", "content": "…UTF-8 text…" },
    { "path": "wiki/old.md", "delete": true }
  ]
}
```

`base` is the head the client last synced, or `null` for the first push. If it is not the
current head the server answers `409` with code `stale_base` and `details.head`, and changes
nothing. Otherwise it validates every path and body (section 3), stores the new content,
creates the revision and moves the head in one transaction, and answers `201` with the
revision. A request over the advertised size answers `413`, and a request that fails
validation answers `422` naming the first offending path. A push is all-or-nothing.

**Pull.** `GET` the repository for `head`; if it equals the stored last-synced revision there
is nothing to do. Otherwise `GET` the head manifest, compare it with the stored last-synced
manifest, fetch only the blobs whose hashes are new, and verify each against its manifest
entry.

**Conflicts** are detected and resolved on the client, because only the client holds the
base, the local edits and the remote edits together. For each path changed on both sides it
runs a three-way line merge. A clean merge is applied. An overlapping one is a **conflict**:

- the local file is left exactly as it is;
- the remote version and the base are written to `.athenaeum/conflicts/<rev>/…`;
- the repository is marked `conflicted` and `push` refuses until each conflict is resolved
  with `athenaeum sync resolve <path> --keep local|remote|both`;
- modify-versus-delete is a conflict, not a deletion.

**Atomicity of a pull.** Files are staged in `.athenaeum/sync/staging/`, every hash is
verified, and only then are they moved into place, with a journal so an interrupted pull is
completed or rolled back on the next run. The last-synced revision is advanced last. The
sync state never claims a revision the working tree does not hold.

**After a pull** the client runs `index-vault`, so search and graph retrieval work locally
from the downloaded text alone.

### Authentication

Passwords never reach the desktop app or CLI. Clients use a device authorisation flow: the
client requests a code, the person approves it in a browser session on Library, and the
client exchanges the approved code for a bearer token. A token has scopes `repo:read`,
`repo:write` or `repo:admin`, may be restricted to named repositories, and is stored hashed
server-side. Clients keep it in the operating system's credential store (Keychain, Credential
Manager, Secret Service), and fall back to a file readable only by its owner with a printed
warning. A token is never logged, never put in a URL, and never sent to any other origin.

A private repository the caller may not read answers `404`, not `403`: its existence is not
disclosed.

### Errors

Every error from every service in this ecosystem has the bridge's shape:

```json
{ "error": { "code": "stale_base", "message": "…", "details": { "head": "rev_…" } }, "requestId": "…" }
```

Codes already used by the bridge keep their meaning (`invalid_request`, `not_found`,
`request_too_large`, `unsupported_protocol`). Library adds `unauthorized`, `forbidden`,
`stale_base`, `conflict`, `invalid_path`, `invalid_content`, `limit_exceeded` and
`rate_limited`. Codes are `snake_case`, stable and part of the contract; messages are for
people and may change.

### Compatibility

`GET /api/v1` reports `apiVersion` and `syncProtocol`. A client refuses a server whose
`syncProtocol` it does not speak, and says so. An unreachable server is the **offline**
state: sync reports it, nothing local changes, and every local operation keeps working.

## 8. The local service and MCP

The protocol v1 bridge is unchanged. The MCP server (a stdio transport first) is a second
front end on the same `WikiService`, not a second implementation. Reads are separate from
writes, and a write needs an explicit permission granted by whoever launched the server.
The default grant is read-only. `athenaeum_sync_repository` is a write for this purpose.
Nothing listens on a network interface unless the user configures it.

## 9. Export and import

Export writes a zip of the repository tree without `.athenaeum/`, with entries in sorted order
and a `MANIFEST.json` of hashes. Import verifies the hashes, refuses absolute paths, `..`
segments and symlinks, never overwrites an existing repository, and finishes with
`index-vault`. A round trip preserves content, ids, relations, sources and authority, because
all of them are text in the tree. Binary raw sources are included in an export even though
they are not in the sync set.

## 10. What belongs where

| Piece | Repository |
| --- | --- |
| Manifest, `id:`, accept/reject, `materialize`, authority cap, indexer, export/import, MCP server, sync client, `athenaeum sync` | `athenaeum` |
| Accept/reject tool surface for the model, proposal prompts | `athenaeum_agent` |
| Accounts, device flow, repositories, revisions, push/compare/blob routes, rendering, discovery, forking | `athenaeum_library` |
| Fixtures for sections 3, 6 and 7 | `athenaeum`, vendored by the other two |

Library is built from this document and the fixtures. It does not import or copy Athenaeum
code: Athenaeum is GPL-3.0, and the protocol is the interface.
