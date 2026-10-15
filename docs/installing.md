# Installing Athenaeum

Athenaeum is one thing to install. It gives you the desktop app, the `athenaeum` command, and the
connection that lets your coding agents (Claude Code, Codex, Cursor) use your project's wiki as they
work. You do it once; after that you code as you always do.

```text
install once ──▶ athenaeum setup (per project) ──▶ use your agent as usual
                                                     │   recalls what the wiki knows
                                                     │   proposes what it learns
                                                     ▼
                       you accept proposals in the app ──▶ pages ──▶ (optionally) the Library
```

## 1. Install with the native wizard

Download the package for your system from the
[latest release](https://github.com/Hanoryx-North/athenaeum/releases/latest):

- **Windows:** open `Athenaeum_<version>_x64-setup.exe`, the native NSIS setup wizard.
- **macOS:** open `Athenaeum_<version>_aarch64.pkg` (Apple silicon) or
  `Athenaeum_<version>_x64.pkg` (Intel) in Apple's Installer. It installs the app in
  Applications; macOS may request administrator authorization. A `.dmg` is also available.
- **Linux:** install the `.deb` or run the `.AppImage`.

Open Athenaeum, open a project, then go to **Knowledge → Library**. Guided setup lets you
choose **Use my existing agent** (Codex, Claude Code or Cursor) or **Use Athenaeum Agent**
(the app chat and `athenaeum-agent` command). Both routes build the same local wiki.
You can connect Athenaeum Library during setup or leave that until later.

The package contains the matching `athenaeum` CLI, a private checksum-verified Node runtime,
and the MCP integration. Base setup copies these resources locally and works offline.
You do not need Node, Rust, a terminal installer command, or a downloaded shell script.
The app invokes its packaged Node entry point directly.

Selecting Athenaeum Agent additionally runs `athenaeum_agent`'s own packaged installer to
install its pinned DeepSeek Harness runtime from npm and mount the packaged wiki bundle.
This step needs internet access. Its `athenaeum-agent` launcher starts the unmodified
Harness with the `athenaeum` profile; coding needs a model API key (`DEEPSEEK_API_KEY`).
The app's chat uses this same installed profile through the Harness SDK.

Open a new terminal after setup. On macOS, setup adds a marked PATH entry to the login
profile for zsh (or bash when it is the user's configured shell). Windows uses the user PATH.
Setup shows progress and errors, can be cancelled, and is safe to retry.

### What it puts where

Everything lives under one directory, and nothing outside it is changed except the launcher links and
your agents' own settings (below).

| Where | What |
| --- | --- |
| `…/Athenaeum/bin/` | `athenaeum` (the command), `athenaeum-integrations`, `athenaeum-mcp` |
| `…/Athenaeum/lib/athenaeum-integrations.mjs` | The agent integration, one file |
| `…/Athenaeum/node/` | A private Node, so the integration keeps working whatever you do to your own |
| `…/Athenaeum/install.json` | What was installed and where |

The directory is `%LOCALAPPDATA%\Athenaeum` on Windows, `~/Library/Application Support/Athenaeum` on macOS
and `~/.local/share/athenaeum` on Linux. On macOS and Linux the three commands are linked into
`~/.local/bin`; on Windows `bin` is added to your user `PATH`.

Advanced script options (after `sh -s --` for the shell installer; as `ATHENAEUM_INSTALL_ARGS` on Windows):
`--prefix DIR`, `--agents claude,codex,cursor`, `--capture off|instruct|nudge`, `--auto-sync`,
`--no-agents`, `--no-link`, and `--cli PATH|URL` / `--integrations PATH|URL` to install builds you
already have.

## 2. Set up a project

```sh
cd your-project
athenaeum setup
```

`setup` is safe to repeat. It creates the project's knowledge base and its `athenaeum.json` identity,
indexes the project's files and symbols (no model is involved), adds `.athenaeum/` to `.gitignore` in a
Git project, and re-checks that your agents are connected. Add `--library` to back the wiki up online
(below). The guided setup in the app does the same project setup after installing the chosen tools.

## 3. Use your coding agent as usual

What changes in each agent:

| | Claude Code | Codex | Cursor |
| --- | --- | --- | --- |
| Tools (`wiki_context`, `wiki_search`, `wiki_create`, …) | MCP server | MCP server | MCP server |
| Knowledge recalled for each prompt | hook | the agent asks | the agent asks |
| Note on when to record knowledge | session-start hook | `AGENTS.md` block | — |
| Housekeeping when a turn ends | hook | — | — |

Codex and Cursor get the tools but not hooks: Athenaeum only writes configuration it has a documented
format for, and Codex's and Cursor's hook formats are not wired.

In a project that has a knowledge base, Claude Code is handed a short brief of what the wiki knows that
relates to your prompt, inside the engine's trust wrapper: reference material, never instruction. In a
project without one, the hooks do nothing and the agent behaves exactly as before. If anything about
Athenaeum fails, a hook stays silent and exits cleanly; a broken wiki never breaks your coding session.

**Storing what the agent learns.** The agent is told to record durable, non-obvious things (why a decision
was made, a trap that cost time, a convention) with `wiki_create`. That only *proposes* it. A person
accepts it, in the app under **Review** or with `athenaeum proposals` / `athenaeum accept`, and accepting
is what writes the Markdown page. Conversation text never becomes permanent knowledge by itself.

`--capture` sets how hard the agent is pushed: `instruct` (default) tells it once per session; `nudge`
also asks it, at most once per session and only after it edited files without proposing anything, whether
there is something worth recording; `off` removes the instructions.

## 4. The Library (optional)

The Library is a GitHub-like store for your *accepted wiki text*, browsable at
<https://wiki.hanoryx.com>. It holds Markdown and the minimal metadata for identity and history, and never
raw sources, code, indexes, agent sessions or credentials.

```sh
athenaeum setup --library          # sign in once in a browser, create a private repository, link it
athenaeum sync                     # pull, then push accepted pages; stops at conflicts
```

Nothing is sent until you sync. If you pass `--auto-sync` to the installer or to `setup`, Claude Code also
syncs at the end of a turn, but only when the project is linked and out of step, at most once a minute, in
the foreground of your own session, and it stops at conflicts or a missing network instead of forcing
anything. It is never a background process.

## Checking and repairing

```sh
athenaeum doctor
```

lists every link (the command, this project, the Library sign-in, each agent's configuration) and the one
thing to do about any that is broken. `athenaeum setup` repairs what `doctor` reports.

## Updating

The desktop app updates itself, including its packaged setup resources. Run **Configure coding agents**
in Knowledge → Library again to update the installed CLI and integrations. Choose Athenaeum Agent to
also update its installed bundle and runtime. Project wikis and credentials are preserved.

## Removing

```sh
node install.mjs --uninstall        # from a checkout; or `athenaeum-integrations disconnect` to only unwire agents
```

Removes exactly what the installer made. Entries `connect` added to your agents' settings are removed
and nothing else in those files is touched; a first copy of each file it edited is kept beside it as
`*.athenaeum-backup`. Your projects' wikis and your Library account are untouched.

## What this changes in your agents' settings

| File | Change |
| --- | --- |
| `~/.claude.json` | adds `mcpServers.athenaeum` |
| `~/.claude/settings.json` | adds three hook entries (`SessionStart`, `UserPromptSubmit`, `Stop`) that run `athenaeum-integrations hook claude …` |
| `~/.codex/config.toml` | adds a delimited `[mcp_servers.athenaeum]` block |
| `~/.codex/AGENTS.md` | adds a delimited note |
| `~/.cursor/mcp.json` | adds `mcpServers.athenaeum` |

(`CLAUDE_CONFIG_DIR` and `CODEX_HOME` are honoured.) Existing entries are preserved. A file that is not
valid JSON, or a hand-written `[mcp_servers.athenaeum]` in Codex's config, is reported and left alone.

## Requirements

- Windows 10 or later, macOS 11 or later, or Linux (glibc), x64 or arm64.
- Native base setup needs no network. Athenaeum Agent runtime setup needs the npm registry;
  connecting or syncing Athenaeum Library needs its server. Advanced script installation also needs GitHub and nodejs.org.
- **While the `athenaeum` repository is private** the release files cannot be downloaded without signing
  in, so the one-liners return a 404. Download the release assets (`athenaeum-cli-<target>.tar.gz` or
  `.zip`, and `athenaeum-integrations.mjs`) and install them with
  `node install.mjs --cli <archive> --integrations <file>`.

## Advanced command-line installation

The existing `install.sh`, `install.ps1` and `install.mjs` remain available for Linux,
automation and CLI-only installs. They are optional alternatives to the native wizards:

```sh
curl -fsSL https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.sh | sh
```

```powershell
irm https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.ps1 | iex
```

These install the engine CLI and external-agent integration. To install Athenaeum Agent
without the desktop, use that repository's own installer. No agent runtime is implemented here.

## Not covered

- **Codex and Cursor hooks.** Their MCP tools work; automatic recall and end-of-turn upkeep are Claude Code only.
- **Platform signing credentials.** Updater signatures are separate from OS code signing.
  Apple app/installer signing and notarization require release credentials; Windows certificate
  signing is not configured. See [releasing.md](releasing.md).
