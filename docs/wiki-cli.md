# Wiki CLI

`wiki` is Athenaeum's standalone command line. It uses the same local engine,
Markdown vault, index, credentials, bridge and sync implementation as `athenaeum`.
Both executable names remain supported. Node and `npx` are unnecessary for local
knowledge operations. Agent/MCP launchers and source builds use their own runtimes.

## Install and start

For local development, launch the compiled desktop from the command line:

```sh
wiki start                 # build once if missing, otherwise just open the app
wiki start --build         # refresh frontend + desktop using the existing cache
wiki start --dev           # optional foreground developer session with live updates
wiki start --dry-run --json
```

This workflow creates no installer packages or desktop shortcuts. It keeps a
ready app in `athenaeum/target/standalone` (or `$CARGO_TARGET_DIR/standalone`).
The frontend is embedded; ordinary starts run no compiler or development server
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
The MCP server then reaches it even when ChatGPT or Codex starts in another folder.
Reconnecting from another wiki changes the selected wiki; `wiki doctor -C DIR`
checks that the saved connection actually addresses it. Routine recall and capture
are quiet; host-controlled tool activity can still be visible. Proposals await
human acceptance in Review before becoming wiki pages.

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
wiki sync push --message "Document callback validation"
```

Creation retains the existing proposal/review lifecycle. A model proposal becomes
durable Markdown only when accepted. `--stated` explicitly records a user-stated
claim. Indexes remain derived and rebuildable. Existing trust and sync rules apply.

The bridge remains a closed knowledge contract; developer process execution and
settings editing are CLI operations and are not exposed as model-facing bridge methods.

## Saved desktop settings

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

Developer commands operate on the sibling source checkouts and delegate to their
existing build tools. They do not create a separate product implementation.
Discover the root from the working directory, set `WIKI_DEV_ROOT`, or pass
`wiki dev --root /path/to/parent …`.

```sh
wiki dev components --json
wiki dev doctor --json
wiki dev tasks --json
wiki dev status
wiki dev install cli,library,agent
wiki dev build cli,library,admin --release
wiki dev check cli,api
wiki dev fmt cli
wiki dev test cli
wiki dev test --suite product-cli,product-engine
wiki dev test --suite product-local-to-cloud -- --results sync-tests.json
wiki dev serve api
wiki dev serve library -- --host 127.0.0.1
wiki dev serve desktop
wiki dev run api reference:typecheck
wiki dev --dry-run build all --release --json
```

Components: `protocol`, `engine`, `cli`, `integrations`, `agent`, `app` (web
frontend), `desktop` (native shell), `library`, `admin`, `api`, `validation`.
Multiple names may be comma-separated or space-separated. Omitted targets mean
`all`. Builds run in dependency order. `--release` applies to native Rust builds.
The retired core and agent runtime are excluded. The Harness stays an upstream
dependency; its normal setup requirements still apply.

`dev tasks` exposes package scripts directly from the checked-out manifests, so
specialized scripts remain reachable through `dev run COMPONENT SCRIPT -- ARGS`.
Native Rust components also support cargo build, fmt and clippy through the named
commands. `dev fmt all` formats native workspaces and packages that declare a
formatter; a component with no formatter is rejected when selected on its own.
The validation component has no build artifact.

`dev test` runs the central validation runner and treats skipped steps as failures.
API selection also runs the production Rust API's own tests. Explicit `--suite`
runs exactly the selected validation steps. Heavy optional suites such as desktop
are selected explicitly. `dev test all` runs the runner's default suites and Rust
API tests; use `--suite app-desktop` for the optional desktop build.

`--dry-run` prints the exact executable, argument array and working directory
without running commands. `--json` keeps child logs on stderr and emits a final
JSON report with exit codes and durations. A failed child stops the plan and
returns nonzero. Servers run in the foreground with inherited Ctrl+C; omit
`--json` for a running server. Nothing starts a background deployment or publishes
a release on its own. Deployment scripts, if present, remain explicit `dev run` tasks.

Install `wiki` outside the checkout's build directory before using it to rebuild
the CLI, particularly on Windows where a running executable cannot be replaced.

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

The current CLI covers the headless engine, saved settings, integrations, hosted
API, and source workflow. Desktop-only rendering and browser-orchestrated ingest,
research and embedding jobs are not implemented as headless CLI jobs here.
