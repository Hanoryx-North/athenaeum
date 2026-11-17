# Athenaeum

A local Markdown wiki with a desktop, CLI, rebuildable knowledge index, and an
integrated coding assistant. Knowledge stays in your files; the agent uses a
model provider you configure and recalls useful project knowledge through the
same versioned bridge as other clients.

Engineering memory supports reviewed lessons from coding work and pasted text;
see the [implementation audit and usage guide](docs/engineering-memory.md).

The desktop UI and wiki workflows derive from
[llm_wiki](https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
See [provenance](docs/provenance.md).

## Install and use

Download a platform installer from the
[latest release](https://github.com/Hanoryx-North/athenaeum/releases/latest).
For local macOS builds through Relay, see [the build contract](docs/relay-build.md).
Open or create a project, add sources, and review the wiki pages and proposals.
Choose a model provider in Settings to use Athenaeum Agent in chat. Local wiki
browsing, search and review need no API key. Library sync is optional.

**Connectors → Coding agents** connects Athenaeum Agent to a selected project wiki;
Claude Code and Cursor remain optional external MCP clients. The built-in assistant
has its own model settings and state. See [installation](docs/installing.md) and
[agent memory](docs/agent-memory.md).

The agent can read/search files, inspect images, propose and apply patches, and run
local development commands after approval. It streams activity into the desktop,
supports interruption, and saves conversations for resuming. Commands run with your
privileges; the focused runtime does not provide an operating-system sandbox.
See [runtime capabilities and limits](packages/agent/runtime/README.md).

## Develop

Read [AGENTS.md](AGENTS.md) and the [one-checkout guide](docs/consolidation.md).
Use Node 22.19+ (CI uses 24), Rust 1.88+ and Python 3.11+.

```sh
npm ci
npm run build:packages
cargo build -p athenaeum-cli
npm run native:build -w athenaeum-agent
npm start
```

`npm start` and `wiki start` use live frontend HMR and incremental native builds.
`wiki start --standalone` deliberately uses a frozen snapshot; use
`npm run desktop:build` to refresh it. Routine development needs no installer or
release optimization. Native desktop compilation also needs the platform's Tauri
build prerequisites and protobuf compiler.

```sh
npm run check:packages
npm test
npm run build
python3 validation/verify_all.py --list
python3 validation/verify_all.py --only protocol,engine,agent,product-cli,native-agent-config,native-agent-smoke --fail-on-skip
```

## Source map

| Path | Owner |
| --- | --- |
| `src/`, `src-tauri/` | React desktop, native commands and agent client |
| `crates/athenaeum-core/` | The single knowledge engine and SQLite index |
| `crates/athenaeum-cli/` | `wiki` and compatible `athenaeum` commands |
| `packages/protocol/` | Versioned TypeScript/Rust contracts and fixtures |
| `packages/integrations/` | MCP server, bridge client and external connections |
| `packages/agent/runtime/` | Athenaeum's own agent, local tools and model loop |
| `packages/agent/src/` | Optional adapter for the separately installed Harness |
| `validation/` | Product and boundary checks against current sources |

The engine has no UI, Tauri or network dependencies. Agent clients use the bridge;
they never open the index or maintain a separate knowledge graph. There are no
required sibling repositories. Hosted Library/API/admin applications are separate
services; the app retains compatible HTTP/sync clients.

## CLI and storage

`wiki commands --json`, `wiki desktop actions --json` and `wiki dev tasks --json`
make operations discoverable. See [the CLI guide](docs/wiki-cli.md).

```sh
cd /path/to/project
wiki init
wiki index-vault
wiki search "oauth callback"
wiki brief "fix the oauth callback bug" --prompt
wiki proposals --json
wiki lint
```

Markdown under `wiki/` is the durable source of truth. `.athenaeum/knowledge.db`
is a rebuildable index; unaccepted proposals live there until review, so include
it in backups if proposals matter. Raw sources, credentials, indexes and agent
sessions are excluded from Library sync. The agent's private state lives separately
under `~/.athenaeum-agent`.

## Licence

GPL-3.0-only for the application, engine and owned native agent runtime.
The protocol, MCP integrations and optional Harness adapter retain their MIT
notices under `packages/`. Dependencies retain their own licences.
