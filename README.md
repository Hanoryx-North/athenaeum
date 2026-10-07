# Athenaeum

A local, persistent knowledge graph for a codebase — and the coding agent that
navigates it.

> **Building on Athenaeum?** Read [`AGENTS.md`](AGENTS.md) first. It defines what this
> repository owns and — just as importantly — what belongs to
> [`athenaeum_agent`](https://github.com/Hanoryx-North/athenaeum_agent) and
> [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) instead.
> [`docs/vision/build-plan.md`](docs/vision/build-plan.md) is cross-repository product
> vision, not this repository's backlog.

The idea, in one sentence: **a coding agent gets much better when it stops
rediscovering the same things about your project every session.** Athenaeum
gives it somewhere to put what it learns — typed knowledge nodes, typed
relationships, evidence pointing back at real source — and a cheap way to walk
back to it later.

Knowledge lives as **markdown files on disk** — an Obsidian-compatible vault,
`[[wikilink]]`-connected — with a SQLite index over it that can be rebuilt at any
time (delete `.athenaeum/knowledge.db` and you lose nothing but time). No server,
no vector database, no local model.

---

## Install

Download the installer for your system from the
[latest release](https://github.com/Hanoryx-North/athenaeum/releases/latest):

- **Windows:** `Athenaeum_<version>_x64-setup.exe`. A normal setup wizard; it installs for your
  user (no administrator prompt) and installs WebView2 for you if your PC lacks it.
- **macOS:** the `.dmg` (`aarch64` for Apple silicon, `x64` for Intel). Drag Athenaeum to
  Applications.
- **Linux:** the `.AppImage` (make it executable and run it) or the `.deb`.

Windows and macOS ask for a one-time approval on first launch because the installers are not yet
signed with a paid certificate: [what to click](docs/releasing.md#first-launch-on-macos-and-windows).

Athenaeum then **updates itself**. A new release is built automatically whenever `main` changes;
the app notices, offers **Update and restart**, and installs it in place. How that works, and the
one-time key setup it needs, is in [docs/releasing.md](docs/releasing.md).

---

## Status

Athenaeum is the **visible layer**: a desktop application a person opens, and the
knowledge library behind it. The agent that maintains the wiki is not in this
repository — see below.

What works:

- **The vault** — `wiki/*.md`, `raw/sources/`, `purpose.md`, `schema.md` — is the
  source of truth. `athenaeum index-vault` turns it into typed nodes, deterministic
  `[[wikilink]]` edges and evidence, reconciling rather than rebuilding, so a page
  edited by the app, by Obsidian or by an agent is picked up and nothing else is
  touched. `athenaeum lint` reports orphans, broken links and dead ends from the
  same index.
- **The knowledge library** — nodes, typed edges, evidence, provenance, authority,
  history — in SQLite, with full-text search that finds a node named by an
  identifier from the words a person would type (`oauth callback` reaches
  `OAuthCallback`), graph navigation, shortest paths and knowledge briefs.
- **Review** — `athenaeum check` lists contradictions, lost source anchors, rules
  resting on nothing, proposals nobody accepted and knowledge that keeps surfacing
  without ever helping. A claim is promoted only against evidence
  (`verify`), and a disagreement is recorded and then settled (`contradict`,
  `supersede`) rather than silently picked.
- **A CLI** (`athenaeum`) covering every operation, with `--json` on all of it.
- **A versioned Wiki integration service and bridge** for an agent runtime: typed in
  Rust through `WikiService`, or JSON over `athenaeum bridge`. A node that is a page
  says which file it is, and the bridge keeps the index current with the vault while
  an agent edits it. See [docs/agent-integration.md](docs/agent-integration.md).
- **Deterministic repository indexing** — files, symbols, local imports, calls,
  tests and Git history — with no model involved.
- **Source-anchor relocation** by symbol and region fingerprint.
- **A desktop shell** (Tauri 2 + React 19) with the Obsidian-style interface ported
  from [llm_wiki](https://github.com/nashsu/llm_wiki): three-column layout, graph
  view, markdown editor and the auto wiki builder that turns dropped sources into
  linked wiki pages. [`docs/porting-llm-wiki.md`](docs/porting-llm-wiki.md) records
  what was copied, what was rewired and what was deliberately refused.

### Where the agent lives

The coding-agent runtime is intentionally **not built in this repository**. It is
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness), which owns
providers, streaming, tools, permissions and sessions, with
[`athenaeum_agent`](https://github.com/Hanoryx-North/athenaeum_agent) as the bundle
that plugs this library into it. Athenaeum exposes a versioned, storage-independent
Wiki contract for that, and the agent reaches the library through nothing else.

---

## Quick start

```bash
npm install
cargo build --release

# The CLI needs no window and no API key.
./target/release/athenaeum --help
```

Point it at a vault — a folder of markdown pages — and ask it something:

```bash
cd ~/notes/my-wiki          # wiki/*.md, raw/sources/, purpose.md …
athenaeum init
athenaeum index-vault       # pages become nodes, [[wikilinks]] become edges
athenaeum search "oauth callback"
athenaeum brief "fix the oauth callback bug"
athenaeum lint              # orphans, broken links, dead ends
```

Or teach it something by hand and ask for it back:

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
crates/athenaeum-core/  the library — knowledge, storage, navigation
crates/athenaeum-cli/   the `athenaeum` command
src-tauri/              desktop backend — vault I/O, ingest, parsing, search
src/                    React frontend — the Obsidian-style interface
```

`athenaeum-core` is the product. It has no UI dependency, no Tauri dependency
and makes no network calls, which is what keeps the three platform builds
identical and lets the whole knowledge model be tested without an API key.
The CLI and the desktop app are both just consumers of it.

New agent/runtime work does not belong here. The runtime connects through the Wiki
contract and never through SQLite or store internals.

Full reasoning: [docs/architecture.md](docs/architecture.md).
Where copied code came from: [docs/provenance.md](docs/provenance.md).

---

## Platforms

One codebase, five bundles, built by
[`.github/workflows/release.yml`](.github/workflows/release.yml):

| Platform | Target | Bundle |
| --- | --- | --- |
| Windows | `x86_64-pc-windows-msvc` | `.exe` (NSIS setup wizard) |
| macOS (Apple silicon) | `aarch64-apple-darwin` | `.dmg` |
| macOS (Intel) | `x86_64-apple-darwin` | `.dmg` |
| Linux | `x86_64-unknown-linux-gnu` | `.deb`, `.AppImage` |
| Linux (ARM) | `aarch64-unknown-linux-gnu` | `.deb`, `.AppImage` |

A local `npm run tauri build` still builds every format (`bundle.targets` is `all`); the release
narrows it, bundles PDFium, and signs the update bundles. See [docs/releasing.md](docs/releasing.md).

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

Rust 1.88+ and Node 20+. Linux additionally needs the webview headers and the
protobuf compiler with its well-known types (LanceDB's build script needs both):

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev \
  librsvg2-dev patchelf protobuf-compiler libprotobuf-dev
```

The `athenaeum` CLI and `athenaeum-core` need none of that: `cargo build --release
-p athenaeum-cli` builds without a window, a webview or a model.

```bash
cargo test --workspace     # offline; no API key required
npm run build              # typecheck + bundle the frontend
```

---

## Where the knowledge lives

```text
purpose.md  schema.md        the vault's governance pages
wiki/                        the pages — the source of truth, plain markdown
raw/sources/                 ingested source material
.athenaeum/
├── knowledge.db             the derived index: nodes, edges, evidence, history, journal
├── blobs/sha256/            content-addressed evidence material
├── exports/                 human-readable views, written on demand
└── cache/                   derived; safe to delete at any time
```

The markdown is what you keep; the index is what makes it answerable quickly,
and it can be dropped and rebuilt (`athenaeum index-vault`) without losing
anything. Knowledge that was only ever proposed over the bridge — a model's
suggestion, a hand-added node — lives in the index alone, so back up
`knowledge.db` if you rely on it.

Note what is absent: no `models/` directory. Knowledge is local; intelligence
is not. Your pages are files and your index is a SQLite file inspectable with any
SQLite browser, and both outlive whichever model provider you happen to be using
this month.

---

## Icons

The app icons are generated, not committed by hand:

```bash
python3 scripts/generate_icons.py
```

They are placeholders — a graph motif — until real branding exists. CI checks
that the committed files still match the generator.

---

## Licence

**GPL-3.0-only.** Athenaeum's UI and wiki engine derive from
[llm_wiki](https://github.com/nashsu/llm_wiki) (GPL-3.0, © Yong Su).
[`docs/provenance.md`](docs/provenance.md) records what came from where.

[`athenaeum_agent`](https://github.com/Hanoryx-North/athenaeum_agent) and
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) remain MIT — separate
repositories in separate processes, connected only by the `athenaeum bridge` protocol.
