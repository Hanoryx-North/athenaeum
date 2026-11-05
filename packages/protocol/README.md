> Maintained in the Athenaeum workspace at `packages/protocol`. Build from the
> repository root with `npm ci && npm run build:packages`; see
> [the current development guide](../../docs/consolidation.md). Older sibling
> checkout instructions below are historical.

# @athenaeum/protocol

The language-neutral wire contracts shared by every Athenaeum repository, plus the fixtures that
prove each implementation agrees with them. It is a library of **types, constants, pure
validators and JSON fixtures**. It owns **no behaviour**: no server, no client, no transport, no
database, no vault, no model, no authorisation decisions. Whoever speaks a contract imports it from
here instead of keeping a private copy.

Licence: MIT.

## The three contracts

| Contract | Version | What it is | Modules |
| --- | --- | --- | --- |
| **Bridge** | `BRIDGE_PROTOCOL_VERSION = 1` | The one-shot JSON protocol of `athenaeum bridge`: one request envelope on stdin, one response envelope on stdout. 21 methods (`capabilities` ... `path`): the node / edge / evidence / brief shapes, the workspace `status`, the `review` queue, indexed `sources`, `index` reports and a bounded `path` between two nodes, plus error codes. The `context.prompt` it returns carries the reference-material trust wrapper. | `src/bridge.ts`, `rust/src/bridge.rs` |
| **Sync** | `SYNC_PROTOCOL_VERSION = 1`, `PORTABLE_FORMAT_VERSION = 1` | How accepted `wiki/**/*.md` pages and the root `athenaeum.json` identity manifest are hosted and exchanged: path / content rules (`validateSyncPath`, `isSyncPath`, `validateSyncContent`), the canonical manifest and tree hash (`canonicalManifest`), the repository id (`repo_[A-Za-z0-9_-]{1,60}`), push request and the `stale_base` error, sync routes (`syncRoutes`). | `src/sync.ts`, `rust/src/sync.rs` |
| **Library API** | `API_VERSION = 1` | Everything else the hosted text backend (`athenaeum_api`) says to its web frontends: repository view (with `role` and owner profile), `Page<T>`, tree / file / revision / comparison, tokens, the device flow, the operator (`admin`) shapes, `libraryRoutes` and `adminRoutes`, the full error-code list with HTTP statuses. | `src/library.ts`, `rust/src/library.rs` |

Text only: nothing here describes parsing pages into nodes, edges or embeddings, or calling a model.

**Pre-release v1 correction:** earlier v1 drafts permitted other text paths, including root
Markdown and raw sources. That policy is incompatible with the owner's accepted-wiki-only rule.
Current v1 rejects those paths; a client built against the broader draft must update before it
can push to this API. `validateSyncPath` checks safe syntax; `isSyncPath` checks the hosted set.

## Using it

```ts
import { bridgeRequest, validateSyncPath, canonicalManifest, adminRoutes } from '@athenaeum/protocol'
import type { RepositoryView, PushRequest, BridgeRequest } from '@athenaeum/protocol'
import { syncRoutes } from '@athenaeum/protocol/sync'        // also ./bridge and ./library
import paths from '@athenaeum/protocol/fixtures/sync/v1/paths.json' with { type: 'json' }
```

`package.json` `exports` serve generated files: `src/*.js` at runtime and
`dist/types/*.d.ts` for types. Both are generated from `src/*.ts` (see below), never edited by
hand. Sibling repositories depend on it with `"@athenaeum/protocol": "file:../athenaeum_protocol"`;
the Rust crate is `athenaeum-protocol` in `rust/`.

## Versioning rule: additive only inside v1

* Version 1 never changes the meaning of anything that already exists. Allowed: new exports, new
  optional fields, new error codes, new routes, new bridge methods. Not allowed: removing or
  renaming an export, a field, a code or a route; making an optional field required; narrowing a
  type; changing a canonical form (the manifest / revision serialisation and the SHA-256 over it).
* A breaking change is a new `/api/v2`, a new `protocolVersion`, and a new fixture directory
  (`fixtures/*/v2`) next to the old one. The `v1` fixtures stay as they are.
* Clients tolerate additions: readers ignore unknown fields and unknown error codes, which is why the
  Rust vocabularies are `&str` slices and not closed enums.
* `API_VERSION`, `SYNC_PROTOCOL_VERSION` and `BRIDGE_PROTOCOL_VERSION` are all `1`.

## Fixtures: one set for everybody

```text
fixtures/
  bridge/v1/   requests.json  portable-requests.json  responses.json  vocabulary.json
  sync/v1/     wire.json  paths.json  extensions.json  manifests.json  revisions.json
  library/v1/  examples.json  routes.json  vocabulary.json
```

* `sync/v1/paths.json` names the safe hosted set; `manifests.json` pins canonical serialization.
  This directory is the single home for shared vectors; a consumer should read these files (through the
  `@athenaeum/protocol/fixtures/*` export or the path) instead of keeping its own copy.
* `sync/v1/revisions.json` pins the revision-id serialisation (`canonicalRevision`), and
  `extensions.json` the hosted path allowlist. `wire.json` holds the sync request / error shapes.
* `bridge/v1/requests.json` and `portable-requests.json` together hold one request fixture per
  method: 18 in `requests.json` plus the caller-attributed `proposals`, `accept` and `reject` in
  `portable-requests.json`. Read both files; each method appears exactly once, and the TypeScript
  and Rust tests fail if the union is not all of `BRIDGE_METHODS`.
* `bridge/v1/responses.json` entries for `navigate` ... `reject` were captured from a real
  `athenaeum bridge` run and then normalised (stable ids, timestamps and paths). The additive
  `status`, `review`, `sources`, `index` and `path` entries are the pinned v1 shapes those
  operations must answer with; the review kind / severity and index scope vocabularies live in
  `bridge/v1/vocabulary.json`.
* `library/v1/examples.json` holds example payloads for the Library API: repository views, push
  request and response, a `stale_base` error, the device flow, tokens (placeholder secrets only),
  and the operator shapes. `routes.json` is the expected output of every route builder.
* `vocabulary.json` files list the closed vocabularies (methods, error codes and statuses, scopes,
  roles) so TypeScript and Rust are checked against the same list.

Each language proves agreement against these files:

* TypeScript (`test/*.test.ts`) validates every fixture against a checker that is *typed against the
  exported interfaces* (`test/shape.ts`): a field added to or removed from `src/*.ts` without
  updating the check is a compile error, and a fixture with a field the contract does not name is a
  failure. Vectors run through the real validators and canonical forms.
* Rust (`rust/tests/fixtures.rs`) deserializes every fixture and requires the exact same JSON back
  from serde, and re-derives the path, manifest, tree-hash and revision-id vectors.

Fixtures contain no real credentials. Token secrets are the literal placeholder
`atn_example_not_a_real_secret`.

## Develop

```sh
npm install
npm run build     # tsc: regenerate src/*.js and dist/types/*.d.ts from src/*.ts
npm run check     # fail if the committed .js / .d.ts differ from tsc's output, tests do not typecheck,
                  # or package.json `exports` points at a missing file
npm test          # node:test over every fixture, plus `cargo test` when cargo is available
npm run verify    # build + check + test
```

* `src/*.ts` is the source of truth. `src/*.js` (what the package serves at runtime, ES2022 modules,
  no bundler) and `dist/types/*.d.ts` are **generated and committed**; edit the `.ts` and run
  `npm run build`. `npm test` runs `scripts/check.mjs`, so a stale generated file fails the tests.
* Development needs Node 22.18 or newer (the tests are `.ts` files run by Node's built-in type
  stripping). Consumers of the package need Node 20 or newer.
* The Rust half runs from `npm test` when `cargo` is on `PATH` (or `CARGO` names it); it uses
  `cargo test --locked --workspace` and a private `CARGO_TARGET_DIR` is the caller's choice. Set
  `PROTOCOL_SKIP_RUST=1` to skip it. Run it directly with `cargo test --workspace`. If cargo is
  missing, the test is reported as skipped with that reason: the Rust half is then **not**
  verified.

## What does not belong here

Anything that decides, stores, sends or enforces: HTTP servers and clients, the sync algorithm,
bridge handlers, the knowledge library, authorisation, rate limiting, model providers. Those live in
`athenaeum` (engine, CLI, bridge), `athenaeum_api` (hosted service), `athenaeum_library` and
`athenaeum_admin` (web frontends), `athenaeum_agent` and `athenaeum_integrations` (adapters).
