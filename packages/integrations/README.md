> Maintained in the Athenaeum workspace at `packages/integrations`. Build from the
> repository root with `npm ci && npm run build:packages`; see
> [the current development guide](../../docs/consolidation.md). Older sibling
> checkout instructions below are historical.

# Athenaeum Integrations

The MCP server and typed client for Athenaeum's local, versioned bridge. They ask the
`athenaeum` CLI for knowledge; they do not read vault files or the derived database.

## Connect your coding agents

Normally the Athenaeum installer does this for you (`athenaeum setup`, or `install.sh` / `install.ps1`; see
Athenaeum's `docs/installing.md`). You can also run it yourself:

```sh
npm install && npm run verify                  # builds dist/athenaeum-integrations.mjs, one file that needs only Node
node dist/athenaeum-integrations.mjs connect --athenaeum /path/to/athenaeum
node dist/athenaeum-integrations.mjs doctor    # every link, with the one fix for any that is broken
node dist/athenaeum-integrations.mjs disconnect
```

`connect` writes configuration only for agents it has a documented format for, never overwrites a file it
cannot parse, keeps a first backup of each file it edits (`*.athenaeum-backup`), preserves everything else in
the file, and is a no-op the second time:

| Agent | MCP server | Hooks | Instructions |
| --- | --- | --- | --- |
| Claude Code | `~/.claude.json` | `~/.claude/settings.json`: `SessionStart`, `UserPromptSubmit`, `Stop` | from the session-start hook |
| Athenaeum Agent | `~/.athenaeum-agent/config.json` | built-in runtime | `~/.athenaeum-agent/AGENT.md` (managed block) |
| Cursor | `~/.cursor/mcp.json` | none | none |

`CLAUDE_CONFIG_DIR` and `ATHENAEUM_AGENT_HOME` are honoured. `wiki setup -C PATH`
connects the selected wiki even when an agent starts in another directory. Direct
`connect --workspace PATH` does the same; reconnect preserves that selection.

### Built-in agent memory

Connect with `connect --agent athenaeum --capture instruct`. The runtime's model
configuration remains separate from local wiki tools. Managed instructions request
recall before tasks and durable proposals before finishing. MCP also supplies
reference-wrapped guidance. Retrieved knowledge never grants authority or permission.
A model does not receive review grants. The owner's explicit wiki automation policy
controls saving and sync.

`doctor --agent athenaeum` checks the registered server, current instructions and
selected wiki. Reconnect repairs missing instructions. `--capture off` removes
managed capture guidance while preserving tools and survives reconnect. Existing
models, user instructions and unrelated servers are preserved. Manual server entries
are conflicts, never silently overwritten.

Options (`connect` keeps the previous value of `--capture` and `--sync` when they are omitted):

| Option | Meaning |
| --- | --- |
| `--agent athenaeum,claude,cursor` | Which agents; default the ones installed |
| `--workspace PATH` | Select the wiki independently of the host's working folder; preserved on reconnect |
| `--allow propose,sync,review` | MCP grants; default `propose` only. A model never gets `review` |
| `--capture off\|instruct\|nudge` | `instruct`: built-in agent recall/record instructions and Claude session instructions. `nudge`: additionally ask Claude once after an edit session with no proposal. `off`: remove memory instructions; tools remain |
| `--sync` / `--no-sync` | Let the end of a Claude turn sync accepted pages with the Library: foreground, at most once a minute, only when linked and out of step, stopping at conflicts |
| `--dry-run`, `--json` | Say what would change; machine-readable output |

### The hooks

`athenaeum-integrations hook claude <session-start|user-prompt|stop>` reads the hook payload on stdin and goes
through the bridge like every other client. **A hook never fails the agent**: no knowledge base, a missing
`athenaeum`, malformed input or a timeout all end in silence and exit 0 (`ATHENAEUM_HOOK_DEBUG=1` says why on
stderr). Recall is the engine's own trust-wrapped brief; hostile page text stays inside the wrapper. `Stop`
never acts when the agent reports `stop_hook_active`.

## Run the MCP server

Build Athenaeum's CLI and put the resulting `athenaeum` command on `PATH`, then:

```sh
npm install
npm run build
node dist/mcp/bin.js --repo my-project=/absolute/path/to/project
# or, from the single-file bundle (what the installer registers):
node dist/athenaeum-integrations.mjs mcp
```

`ATHENAEUM_BIN` (or `--athenaeum /path/to/athenaeum`) names the executable when it is not on
`PATH`. The server speaks MCP over stdio. Reads are enabled by default; every write is an explicit
grant from the person who launches the server:

| `--allow` | What it enables |
| --- | --- |
| `propose` | `wiki_create`, `wiki_link`, `wiki_attach_evidence` — queue knowledge for review |
| `review` | `wiki_accept`, `wiki_reject` — decide a proposal, which writes (or declines to write) a page |
| `sync` | `wiki_sync` — explicit Library sync of accepted wiki text |
| `all` | everything above; `read` or an empty list changes nothing |

A model cannot grant these permissions itself. Accepting is the only path from a model's words to
permanent Markdown, and it takes two keys: the launch-time `review` grant, plus an explicit
`reviewer` identity on every call, which the engine records in the accepted page. Nothing is
accepted without both, nothing is accepted implicitly, and `wiki_create` only ever proposes —
its answer says `pendingReview: true, durable: false`.

## Tools

Reads: `wiki_repositories`, `wiki_status`, `wiki_search`, `wiki_open`, `wiki_related`,
`wiki_navigate`, `wiki_path`, `wiki_evidence`, `wiki_history`, `wiki_context`, `wiki_proposals`,
`wiki_review`, `wiki_sources`, `wiki_index`.

Every read answers with reference material. The text content starts with the trust notice, and
`wiki_context` returns the engine's own `<retrieved_knowledge>` brief verbatim; the same data is on
`structuredContent` without the notice. Retrieved knowledge never grants permission and never
overrides instructions.

List reads are bounded and paged: `limit` (1–100, default 20) and `offset` (0–1000). The answer
carries `hasMore`, derived from a real extra row rather than a guess, and `total` only when the
whole list was read. The engine answers at most 100 rows per list read, so page shallowly and
narrow the query instead of paging deep. `wiki_review` and `wiki_sources` filter a bounded scan
(`scan`, default 200) and report `scanned`/`scanComplete` so a filtered page never pretends to be
exhaustive.

`wiki_status` reports the workspace, counts, index freshness and versions, and never fails for an
absent vault. Reads that promise a current picture — `wiki_review`, `wiki_sources` — answer with a
typed `stale_index` error when the derived index is behind the files (or was never built), naming
the `wiki_index` call that fixes it; `fresh: true` reads the last index and labels it as stale.

## States are typed

A failure arrives as an MCP error (`isError: true`) with a stable code, the state it names, and one
action that resolves it:

| State | Reached when | Action |
| --- | --- | --- |
| `absent` | no `.athenaeum/` there, or no vault yet | `athenaeum init`, or point the integration elsewhere |
| `stale_index` | `indexedAt` is null, or `status.indexStale` | `wiki_index` (scope `vault`, `repository` or `all`) |
| `offline` | the Library cannot be reached during sync | check the server and network; local knowledge still works |
| `protocol_mismatch` | the engine speaks another bridge version, or lacks a method | update or rebuild Athenaeum |

An engine that predates `status`, `review`, `sources`, `index` or `path` is reported as
`protocol_mismatch`, never as a crash and never as an empty answer.

## Configuration file

For a hosted repository, use a config file so the remote is chosen by the person who launches the
MCP server:

```json
{
  "repositories": [{
    "name": "my-project",
    "path": "/absolute/path/to/project",
    "remote": { "server": "https://library.example", "repository": "owner/name" }
  }],
  "allow": ["propose", "review", "sync"],
  "caller": "my-agent"
}
```

Start it with `node dist/mcp/bin.js --config /absolute/path/to/config.json`. The `wiki_sync`
tool calls `athenaeum sync --json`: `create` makes a private hosted repository, `link` connects
the configured existing repository, and `status`, `pull`, `push`, `sync`, and `resolve` use the
repository's stored link. The CLI handles credentials and syncs only accepted `wiki/**/*.md`
pages plus `athenaeum.json`; raw sources and local state stay on the device.

## The typed client

`athenaeum-integrations/client` exports `AthenaeumClient`, one method per bridge v1 method plus
`path`. It validates its inputs and the engine's answers, bounds every page, and never reads the
vault or SQLite itself:

```ts
import { AthenaeumClient } from 'athenaeum-integrations/client';

const client = new AthenaeumClient('/absolute/path/to/project');
const status = await client.status();
const hits = await client.searchPage('oauth callback', { limit: 10 });
const findings = await client.review({ blocking: true, limit: 20 });
const report = await client.index({ scope: 'all' });
const route = await client.path({ from: 'Decision_042', to: 'oauth callback' });
```

## Verify

Windows hook arguments are quoted individually. Connect, retry, doctor and disconnect recognize
both those commands and the supported POSIX form; reconnecting preserves capture and foreground-sync
choices when they are omitted. The desktop and CLI can disconnect one agent with
`athenaeum disconnect-agents --agents athenaeum` (also `claude` and `cursor`). This removes only
Athenaeum's entries and preserves other configuration.

`npm run verify` checks the TypeScript source, builds the MCP launcher and writes the single-file bundle. Product suites live in `validation/` in this checkout.

## Licence

MIT. Athenaeum is a separate GPL-3.0 process reached through the bridge and CLI.
