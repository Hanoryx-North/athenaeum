# Athenaeum

A local, persistent knowledge graph for a codebase — and the coding agent that
navigates it.

The idea, in one sentence: **a coding agent gets much better when it stops
rediscovering the same things about your project every session.** Athenaeum
gives it somewhere to put what it learns — typed knowledge nodes, typed
relationships, evidence pointing back at real source — and a cheap way to walk
back to it later.

Everything lives in one SQLite file inside your project. No server, no vector
database, no local model.

---

## Status

This is the **base application**: the knowledge library, a CLI, and a desktop
shell that runs on Windows, macOS and Linux. It is genuinely usable today for
reading and writing knowledge by hand.

What works:

- The knowledge model — nodes, typed edges, evidence, provenance, authority,
  history — persisted in SQLite with full-text search.
- Graph navigation, bidirectional traversal, shortest paths, knowledge briefs.
- A CLI (`athenaeum`) covering every operation.
- A desktop shell (Tauri 2 + React) for browsing and editing.
- A provider-agnostic model client with per-role routing.
- A versioned Wiki integration service and cross-language CLI bridge.
- Conservative curator entity resolution for identifier-style lookalikes.
- Deterministic repository indexing for files, symbols, local imports and Git
  history, with no model involved.
- Source-anchor relocation by symbol and region fingerprint.

The coding-agent runtime is intentionally **not built in this repository**.
It is a separate foundation repository that owns providers, streaming, tools,
permissions and sessions. Athenaeum exposes a versioned, storage-independent
Wiki contract for it: typed in-process Rust through `WikiService`, or JSON
over the `athenaeum bridge` command. See
[docs/agent-integration.md](docs/agent-integration.md).

---

## Quick start

```bash
npm install
cargo build --release

# The CLI needs no window and no API key.
./target/release/athenaeum --help
```

Teach it something, then ask for it back:

```bash
cd ~/code/my-project
athenaeum init
athenaeum index

athenaeum add --type decision --id D_042 "Validate OAuth state first" \
  --summary "State is validated before any session is created." \
  --authority project_decision --stated

athenaeum add --type gotcha "Session-before-validation" \
  --summary "Creating the session first opens a login CSRF." \
  --authority project_rule --stated

athenaeum link D_042 has_gotcha Session-before-validation --derived "user stated"
athenaeum evidence D_042 --file src/auth/oauth.ts --symbol OAuthCallback.handle

# Later, in a different session, from a task description:
athenaeum brief "fix the oauth callback bug"

# After refactoring source, relocate evidence anchors and update staleness:
athenaeum relocate
```

```text
# Knowledge brief

Task: fix the oauth callback bug

## Relevant knowledge
1. Validate OAuth state first (decision, project_decision)
   State is validated before any session is created.
2. Session-before-validation (gotcha, project_rule)
   Creating the session first opens a login CSRF.

## Relationships
Validate OAuth state first → has_gotcha → Session-before-validation

## Source anchors
src/auth/oauth.ts:OAuthCallback.handle

## Warnings
- Session-before-validation: Creating the session first opens a login CSRF.
```

`athenaeum brief --prompt` wraps the same thing in the trust boundary a model
should receive: retrieved knowledge is *reference material*, never instruction.

### Desktop app

```bash
npm run tauri dev      # develop
npm run tauri build    # produce an installer for the current platform
```

---

## How it is put together

```text
crates/athenaeum-core/       the library — knowledge, storage, navigation
crates/athenaeum-providers/  model access (Anthropic, OpenAI, Google, …)
crates/athenaeum-cli/        the `athenaeum` command
src-tauri/                   desktop shell (thin)
src/                         React frontend (thin)
```

`athenaeum-core` is the product. It has no UI dependency, no Tauri dependency
and makes no network calls, which is what keeps the three platform builds
identical and lets the whole knowledge model be tested without an API key.
The CLI and the desktop app are both just consumers of it.

The provider crate is bootstrap compatibility code from before the runtime was
split into Repository 2. New agent/runtime work does not belong here. The
future runtime connects through the Wiki contract and never through SQLite or
store internals.

Full reasoning: [docs/architecture.md](docs/architecture.md).
Where copied code came from: [docs/provenance.md](docs/provenance.md).

---

## Platforms

One codebase, five bundles, built by
[`.github/workflows/release.yml`](.github/workflows/release.yml):

| Platform | Target | Bundle |
| --- | --- | --- |
| Windows | `x86_64-pc-windows-msvc` | `.msi`, `.exe` (NSIS) |
| macOS (Apple silicon) | `aarch64-apple-darwin` | `.dmg` |
| macOS (Intel) | `x86_64-apple-darwin` | `.dmg` |
| Linux | `x86_64-unknown-linux-gnu` | `.deb`, `.AppImage` |
| Linux (ARM) | `aarch64-unknown-linux-gnu` | `.deb`, `.AppImage` |

The differences between them are confined to `src-tauri/tauri.*.conf.json`.
Two choices do the heavy lifting:

- **SQLite is compiled from source and linked statically** (`rusqlite`'s
  `bundled` feature), so every platform gets the same engine with FTS5
  present, rather than whatever the host happens to ship.
- **TLS is rustls, not native-tls**, so there is no OpenSSL to find and no
  per-platform TLS behaviour to debug.

A knowledge base created on one platform opens unchanged on another. Anchored
file paths are normalized to forward slashes on the way in for the same
reason.

### Building locally

Rust 1.82+ and Node 20+. Linux additionally needs the webview headers:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf
```

```bash
cargo test --workspace     # offline; no API key required
npm run build              # typecheck + bundle the frontend
```

---

## Where the knowledge lives

```text
.athenaeum/
├── knowledge.db     nodes, edges, evidence, history, journal
├── blobs/sha256/    content-addressed evidence material
├── exports/         human-readable views, written on demand
└── cache/           derived; safe to delete at any time
```

Note what is absent: no `models/` directory. Knowledge is local; intelligence
is not. It is your SQLite file — inspectable with any SQLite browser, and it
outlives whichever model provider you happen to be using this month.

---

## Icons

The app icons are generated, not committed by hand:

```bash
python3 scripts/generate_icons.py
```

They are placeholders — a graph motif — until real branding exists. CI checks
that the committed files still match the generator.
