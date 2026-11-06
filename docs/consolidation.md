# One Athenaeum checkout

The desktop, CLI, knowledge engine, protocol, integrations, optional Harness
adapter and validation live in this repository. No sibling checkout is a local
build dependency; the separate `athenaeum_core` repository is left alone.
`docs/consolidation.json` records the original component imports.

The built-in coding assistant is owned source under `packages/agent/runtime/src`.
A dependency-free Rust launcher embeds those Node modules. Build it with
`npm run native:build -w athenaeum-agent`; output is under
`target/native-runtime/debug`. The desktop is an app-server client, not a second
agent loop. The optional Harness adapter remains separately selectable, with its
external release pinned in `packages/agent/harness.pin.json`.

There is one npm workspace lockfile. The app/engine/CLI use the root Cargo workspace;
the tiny agent launcher has an isolated lockfile with no third-party dependencies.
`npm run build:packages` builds protocol before consumers. Agent contracts alias
the shared protocol, and tests read current generated fixtures without duplicate
source snapshots. MIT components keep their notices. The app and owned runtime
remain GPL-3.0-only.

All knowledge access uses the versioned bridge. Markdown remains the durable wiki;
agent state is isolated under `~/.athenaeum-agent`. The hosted Library/API/admin
services are not local build dependencies. Portable formats and sync exclusions
remain compatible with deployed servers.

`python3 validation/verify_all.py --list` discovers current checks. `--fail-on-skip`
makes missing prerequisites visible. Runtime smoke tests use a local scripted
provider. Native platform packaging and the optional external Harness require
separate prerequisites; ordinary product tests do not establish those results.

Use `npm start` or `wiki start` for live development. Native edits need incremental
compilation. `wiki start --standalone` deliberately opens the frozen snapshot.
See [the cleanup ledger](cleanup-ledger.md) for the runtime replacement, verification
and remaining platform limits.
