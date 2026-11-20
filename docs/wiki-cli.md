# Wiki CLI

`wiki` is Athenaeum's standalone command line. It uses the same local engine,
Markdown vault, index, credentials, bridge and sync implementation as `athenaeum`.
Both executable names remain supported. Node and `npx` are unnecessary for local
knowledge operations. Agent/MCP launchers and source builds use their own runtimes.

## Install and start

Source development defaults to a live desktop:

```sh
wiki start                 # start or reuse the managed Vite/Tauri session
wiki start --dev           # foreground live session with build/HMR logs
wiki start --standalone    # deliberately open the frozen offline snapshot
wiki start --build         # refresh that snapshot using the incremental cache
wiki start --onboarding    # replay the opening flow; keep saved projects and account
wiki start --dry-run --json
```

`npm start` uses the same persistent session and returns after launching it;
repeating either command reuses it. `npm start -- --foreground` attaches build
output when starting a new session. The managed session writes logs to
`.athenaeum-dev/live.log`. Frontend edits
hot-update the running webview. Rust changes rebuild incrementally and restart
the native process. `wiki desktop status --json` reports whether the running app
is live. Do not leave source changes verified only in a bundle while the user
is looking at a stale snapshot.

Pin a launcher that runs `wiki start` from the checkout to reopen the live app
from the macOS Dock. `wiki desktop show` brings the existing window forward.
The explicit standalone workflow keeps a
ready app in `athenaeum/target/standalone` (or `$CARGO_TARGET_DIR/standalone`).
Its frontend is embedded; standalone starts run no compiler or development server
and return immediately while the app stays open. Source changes take effect after
`wiki start --build` (quit the running desktop first on Windows). Keep the Cargo
cache to reuse dependencies; no `cargo clean` is needed. Preparation uses an
incremental debug build, without the full-program release optimization that can
spend minutes on the final linking step. Live development may replace the debug
executable but cannot replace the standalone snapshot. An existing release app
is reused if no snapshot exists. The first build needs the normal desktop
build prerequisites and dependencies (`wiki dev install desktop`).

Run from the source workspace, pass `--root /path/to/parent`, or save
`WIKI_DEV_ROOT` in your shell environment to launch from any directory.
If Cargo config selects a custom target directory, set `CARGO_TARGET_DIR` to
the same directory for the launcher. `wiki dev build desktop --release` also
builds the frontend and native executable without bundling installers.

The native installers and CLI archives include `wiki` alongside `athenaeum`.
The CLI installer exposes both on PATH. To bootstrap from source:

```sh
cargo build --release -p athenaeum-cli
# target/release/wiki (Windows: target/release/wiki.exe)
wiki setup --yes --no-library --no-agents
wiki status --json
wiki commands
wiki commands --json
```

`wiki` without arguments shows help. General wiki operations are the default;
agent chat is an explicit `wiki agent` command. `-C DIR` selects a project;
otherwise commands discover the nearest ancestor with `.athenaeum/`.

`wiki setup -C DIR` also binds the coding-agent connection to that selected wiki.
The MCP server then reaches it even when an agent starts in another folder.
Reconnecting from another wiki changes the selected wiki; `wiki doctor -C DIR`
checks that the saved connection actually addresses it. Routine recall and capture
are quiet. Completed proposals save automatically as Markdown pages.

`wiki commands --json` generates the full command and option catalogue from the
parser, including aliases, accepted enum values and defaults. Use any command's
`--help` for its exact arguments. It is the authoritative command reference.

## Local knowledge and portable repositories

| Area | Commands |
| --- | --- |
| First run | `init`, `setup`, `doctor`, `disconnect-agents` |
| Identity | `repo init`, `repo info` |
| Knowledge | `add`/`create`, `show`/`open`, `list`, `edit`, `status`, `remove`/`delete` |
| Relationships and evidence | `link`, `evidence`, `verify`, `supersede`, `contradict` |
| Retrieval | `search`, `navigate`, `path`, `brief`, `history`, `journal`, `stats` |
| Indexing and quality | `index`, `index-vault`, `sources`, `relocate`, `lint`, `check`, `review` |
| Proposal review | `proposals`, `accept`, `reject`, `draft-source` |
| Portability | `export`, `import`, `vault materialize`, `import-legacy-core` |
| Arrived knowledge | `trust list`, `trust adopt` |
| Library credentials | `library sign-in --stdin` (account JSON on stdin), `library login` (browser device flow), `library logout`, `library status` |
| Sync | `sync`, `sync link`, `sync clone`, `sync status`, `sync pull`, `sync push`, `sync conflict`, `sync resolve`, `sync unlink` |
| Integrations | `agent -- …`, `mcp -- …`, plus `setup`'s agent connection options |
| Automation | `commands`, `completions`, `--json`, `bridge` |

```sh
wiki create "Why callbacks validate state" --content "Reject a state mismatch before creating a session."
wiki proposals --json
wiki accept NODE_ID --as your-name
wiki search callback state --json
wiki index --scope all
wiki sync status --json
wiki sync backup --server https://wiki.hanoryx.com
wiki sync push --message "Document callback validation"
```

`--stated` immediately records a user-stated claim. A model proposal becomes
durable Markdown when the engine accepts it. Indexes remain derived and rebuildable.

The bridge remains a closed knowledge contract; developer process execution and
settings editing are CLI operations and are not exposed as model-facing bridge methods.

## Saved desktop settings

Per-wiki sync settings are stored in `.athenaeum/automation.json`:

```sh
wiki automation --json
wiki automation --auto-sync true
wiki automation --accept-all --json
```

Automatic saving waits 15 seconds for capture tools to attach evidence and links.
The desktop checks every 15 seconds. Bridge knowledge reads also save settled
proposals. Log shows recent saves and failures. Acceptance preserves a claim’s
existing verification status.
Page-to-page `wiki link` and bridge links update the Markdown itself.

After account sign-in, the desktop backs up the open wiki to a private repository
and keeps it synchronized every minute while Athenaeum is running, including when
minimized. Opening another wiki starts its backup too. An explicitly saved pause
is preserved. Disconnecting a wiki pauses automatic backup to prevent relinking.

`wiki sync backup --server URL` performs the same operation without the desktop:
reuse the saved link or create a private repository, enable automatic sync, pull,
then push. `--automatic` respects a saved pause. Repository identity makes retrying
safe and keeps separate wikis with the same name distinct. Existing links keep their
server and visibility. The command refuses to replace a link to another server.

Sync includes `wiki/**/*.md`, root Markdown documents such as `README.md`,
`purpose.md` and `schema.md`, and `athenaeum.json`. Raw sources, proposals, credentials,
local automation settings, indexes, agent sessions and logs stay local. Conflicting
edits wait for review. A completed sync is a server copy, and server snapshot
retention is managed separately by the server operator.

The built-in chat connects to the native Athenaeum Agent app-server. Its provider
configuration lives in `~/.athenaeum-agent/config.json`; supported model routes
chosen in desktop Settings are passed to that isolated process. Keys travel in
its environment rather than conversation files. The
optional Harness integration remains selectable in chat. The terminal stays an
advanced entry point. Agent state and credentials are private to Athenaeum.

`wiki config` reads and writes the desktop's **actual** `app-state.json`:

```sh
wiki config path
wiki config list --json
wiki config get llmConfig.model --json
wiki config set llmConfig.model model-name
wiki config set llmConfig.temperature 0.3
wiki config set generalConfig.closeBehavior exit
wiki config set proxyConfig.enabled true
wiki config set /providerConfigs/my.preset/model model-name
wiki config set llmConfig.apiKey --stdin
wiki config set sourceWatchConfig --value-file watch-settings.json
wiki config unset llmConfig.temperature
wiki config export settings-backup.json
wiki config import settings-backup.json
```

Quit the desktop before writing settings, then reopen it. The desktop keeps an
in-memory settings snapshot; this is a saved-settings interface, not live UI control.
The CLI reports that writes apply on the next desktop launch. These commands edit
saved values; OS integrations such as enabling autostart still need the desktop to
apply them. Use `--file PATH` or `WIKI_SETTINGS_FILE` for a separate profile.

Values parse as JSON when valid and otherwise become strings. Arrays and objects
can be replaced as a whole. Dotted keys address nested objects; JSON Pointers
support keys containing dots or slashes. The CLI preserves unrelated settings,
refuses malformed JSON, keeps the first original as `*.json.wiki-backup`, locks
concurrent CLI writers, and replaces the file atomically. It does not impose a
second schema on the desktop's settings; use values supported by your app version.

Reads/exports redact keys, tokens, passwords and custom headers. Use `--reveal`
explicitly to read or export secrets. Import merges objects and ignores redacted
placeholders. Prefer stdin or a protected file when entering credentials.

## Hosted features and operator access

`wiki api` uses the same Library credentials and HTTP transport as sync. It supports
the entire API v1 surface without waiting for a CLI wrapper for each new endpoint:

```sh
wiki api routes
wiki api routes repository --json
wiki api routes --spec openapi.json --json
wiki api schema
wiki api get user --json
wiki api get repositories --query limit=20 --json
wiki api post repositories --body-file new-repository.json --json
wiki api patch repositories/REPOSITORY_ID --body-file changes.json --json
wiki api delete repositories/REPOSITORY_ID --json
wiki api request options repositories --json
wiki api post repositories --body-file new-repository.json --dry-run --json
```

`routes` reads the server's authoritative OpenAPI catalogue and includes operation
parameters, request bodies, security requirements and component schemas in JSON.
Paths are API-relative, optionally prefixed `/api/v1/`; query values use repeatable
`--query key=value`. `--server URL` or `ATHENAEUM_LIBRARY_URL` selects the origin.
The default Library proxies API calls; its public OpenAPI catalogue is read from
`https://api.hanoryx.com` without credentials. `--schema-server URL` overrides that
catalogue origin for other proxy deployments.
`--stdin` reads a JSON body without putting it in shell history. Responses contain
the HTTP `status` and `body`; failed HTTP responses produce an error and nonzero exit.

Tokens go only to the selected origin; redirects are refused. The server enforces
repository roles, scopes and operator access. Developer commands grant no hosted
privilege. Some account routes require a browser session rather than an API token;
`routes --json` exposes the server's documented security requirements. Direct API
mutations, like direct use of the service itself, happen immediately when invoked.

## Work on Athenaeum itself

Developer commands operate on components inside this source checkout. Discover
its root from the working directory, set `WIKI_DEV_ROOT`, or pass
`wiki dev --root /path/to/athenaeum …`. The old parent-root form is also accepted.

```sh
wiki dev components --json
wiki dev doctor --json
wiki dev tasks --json
wiki dev status
wiki dev install
wiki dev build protocol,integrations,cli,app
wiki dev build agent
wiki dev check cli,integrations
wiki dev fmt cli
wiki dev test cli
wiki dev test --suite product-cli,product-engine
wiki dev test --suite native-agent-config,native-agent-smoke
wiki dev serve app -- --host 127.0.0.1
wiki dev serve desktop
wiki dev run agent native:verify-source
wiki dev --dry-run build all --release --json
```

Components: `protocol`, `engine`, `cli`, `integrations`, `agent`, `app` (web
frontend), `desktop` (native shell), and `validation`. Comma-separated or multiple
names are accepted; omitted targets mean `all`. There is one npm dependency
installation and root lockfile. Builds run in dependency order. Agent builds
compile the TypeScript adapter and the one source native runtime; `--release`
optimizes native builds. The separate `athenaeum_core` repository is excluded.
Hosted projects are not local build targets; the deployed Library client remains.

`dev tasks` exposes each component's package scripts. Native runtime commands such
as `native:build`, `native:check` and `native:verify-source` are available through
`dev run agent SCRIPT`. The native source has an intentionally isolated manifest
with no third-party launcher dependencies. Node runtime modules are embedded into the executable.

`dev test` runs `validation/verify_all.py` against current sources and fails on
skipped tests. Heavy platform/upstream runtime suites are explicit. Use
`--suite app-desktop` for native desktop compilation and `--suite harness-runtime`
with `DSH_REPO` for the optional upstream Harness test. Validation holds the product
suite; unit tests live next to their owners. There is no scratch reconstruction
from historical Git revisions.

`--dry-run` prints executables, arguments and working directories without running
them. `--json` puts child logs on stderr and returns a JSON execution report.
Failures stop the plan. Development servers stay in the foreground; omit `--json`
when starting one. These commands do not deploy or publish releases.

Use an installed `wiki` or `cargo run` to rebuild the CLI on Windows, where a
running executable cannot be replaced. See [the consolidation guide](consolidation.md).

## Completion and integration launchers

```sh
wiki completions powershell
wiki completions bash
wiki completions zsh
wiki completions fish
wiki completions elvish
wiki agent -- --help
wiki mcp -- --allow propose
```

Completion is generated from the same parser as command discovery. Agent and MCP
stream their own protocols and reject the outer `--json` flag. Launchers come
from the install directory or PATH, or `ATHENAEUM_AGENT_BIN` /
`ATHENAEUM_INTEGRATIONS`. No extra agent runtime is implemented in this CLI.

## Running desktop operations

Every desktop operation must be reachable through CLI/API access. Agents must
not control the user's PC to operate or inspect Athenaeum. Read the bundled
[Athenaeum skill](../skills/athenaeum/SKILL.md) for command selection.

```powershell
wiki desktop status --json
wiki desktop actions --json
wiki desktop actions fs. --json
wiki desktop state wiki --json
'["graph"]' | wiki desktop call wiki.setActiveView --stdin --json
wiki desktop call graph.snapshot --json
'["C:/path/to/project", {"maxDepth":3}]' | wiki desktop call fs.listDirectory --stdin --json
'["C:/path/to/page.md"]' | wiki desktop call fs.readFile --stdin --json
wiki desktop reload
wiki desktop quit
```

`call` accepts a positional JSON argument array. `invoke` accepts named JSON
arguments for a registered native command. Use `--stdin` in PowerShell to avoid
native argument quote stripping, and for credentials so they stay off argv.
`--args` works when the caller preserves JSON quotes. Both commands support
`--json`. `actions` includes generated argument signatures and schemas.

The catalog comes from TypeScript exports, Zustand state methods and registered
Tauri commands. Callback-based APIs with required non-JSON arguments are excluded;
their domain operation needs a JSON-compatible action. App, graph, chat and editor
lifecycles register extra actions while those surfaces are mounted. For example,
switch to the graph, wait for `graph.snapshot` to appear, then inspect the actual
Sigma graph and rendered attributes. State inspection is not a screenshot review.

`project.open` and `project.create` run the full desktop project lifecycle.
`project.create` accepts `["Wiki name"]` to use the default parent folder, or
`["Wiki name", "/custom/parent"]` to choose one. New and cloned desktop wikis
default to `~/Library/Application Support/Athenaeum/Wikis` on macOS,
`%LOCALAPPDATA%\Athenaeum\Wikis` on Windows, and
`${XDG_DATA_HOME:-~/.local/share}/Athenaeum/Wikis` on Linux. Existing wikis keep
their saved locations. `fs.defaultWikiDirectory` returns the resolved default.
`fs.openProject` only returns project metadata. Queue services run in the desktop
and use its existing processors; they are CLI-accessible while the app is running,
not new headless model runtimes. Long chat turns return a start acknowledgement;
read `desktop state chat` for their progress and output.

Library sign-in opens Log in the current or most recent wiki. A fresh sign-in
creates `My Wiki` in the default data directory. Use these live actions from the CLI.

```sh
wiki desktop call account.openLog --json
wiki desktop call wiki-notes.openDailyNote --json
printf '%s\n' '["Reading notes"]' | wiki desktop call wiki-notes.createNote --stdin --json
```

`account.openLog` resumes a saved session. `account.signIn` accepts username and
password via stdin. New notes use `wiki/notes/`. Daily notes use `wiki/daily/`
and the local calendar date. Reopening a daily note preserves its contents.

Standalone knowledge commands still work without a desktop. `wiki unlink EDGE_ID`
disconnects saved page relationships in their canonical Markdown and updates the
derived index. It preserves code examples and replaces prose wikilinks with their
display text. `wiki link` creates the corresponding saved relationship.

The live transport binds only to IPv4 loopback on an ephemeral port. A private
`desktop-control.json` beside `app-state.json` carries the same-user bearer token.
The CLI rejects remote endpoints, ignores proxy variables, and follows no
redirects. The server refuses browser-origin requests and oversized bodies.
Do not print the descriptor. State and results redact credential fields by
default; `--reveal` is explicit. A timeout leaves mutation outcome unknown, so
inspect state before retrying. Saved `wiki config` writes still require closing
the desktop; use its live state methods and matching save service for live edits.

Adding a UI feature requires exposing its domain operation in a cataloged service
or store, or through `registerDesktopAction`. Generated discovery prevents a copied
command list drifting. It does not prove that every historical button has been
behaviorally tested; validate the relevant UI/CLI path together.
