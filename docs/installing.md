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

## 1. Install

**macOS and Linux**

```sh
curl -fsSL https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.sh | sh
```

**Windows** (PowerShell, no administrator prompt)

```powershell
irm https://github.com/Hanoryx-North/athenaeum/releases/latest/download/install.ps1 | iex
```

Open a new terminal afterwards. This installs the command line tool and the agent integration, and
connects whichever of Claude Code, Codex and Cursor you have. It does not need Node, Rust or anything
else installed first: it brings its own private Node and checks its checksum.

**Prefer the app?** Download the installer for your system from the
[latest release](https://github.com/Hanoryx-North/athenaeum/releases/latest) (see the
[README](../README.md#install)). Open a project, go to **Knowledge → Library**, and press
**Install the command-line tool**: that runs the same installer, so the app and the terminal
cannot drift apart.

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

Options (after `sh -s --` for the shell installer; as `ATHENAEUM_INSTALL_ARGS` on Windows):
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
(below). In the app, opening a project and pressing **Connect my coding agents** does the same.

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

Run the installer again. It replaces the command and the integration and re-points your agents at them,
and it leaves your projects and credentials alone. The desktop app updates itself.

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
- Access to github.com and nodejs.org while installing.
- **While the `athenaeum` repository is private** the release files cannot be downloaded without signing
  in, so the one-liners return a 404. Download the release assets (`athenaeum-cli-<target>.tar.gz` or
  `.zip`, and `athenaeum-integrations.mjs`) and install them with
  `node install.mjs --cli <archive> --integrations <file>`.

## Not covered

- **Codex and Cursor hooks.** Their MCP tools work; automatic recall and end-of-turn upkeep are Claude Code only.
- **Code signing.** The scripts are fetched over HTTPS from a GitHub release and the private Node is checksum-pinned,
  but the scripts themselves are not signed.
