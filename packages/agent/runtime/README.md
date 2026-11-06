# Athenaeum Agent runtime

Athenaeum's own coding assistant, maintained here as six Node modules and a small
Rust platform launcher. It is a feature of the Markdown wiki application. It has
no copied coding application, terminal UI framework, cloud task service, voice
stack or upstream agent build graph. The launcher embeds the modules, extracts
them into a private temporary directory, runs Node, and removes that directory
on normal exit. Packaged desktops supply their private Node executable.

## Ownership

| Source | Responsibility |
| --- | --- |
| `src/agent.mjs` | Model/tool turn loop, project instructions and conversation state |
| `src/tools.mjs` | File read/write/patch, literal search, listing, images and shell execution |
| `src/provider.mjs` | Responses, OpenAI-compatible Chat Completions and Anthropic streaming |
| `src/mcp.mjs` | Bounded stdio MCP client; optional proposal-only wiki connection |
| `src/config.mjs` | Home, configuration, model validation and atomic session writes |
| `src/cli.mjs` | Interactive/headless entry points and desktop JSON-line service |
| `native-cli/src/main.rs` | Dependency-free platform executable; no second agent loop |

The desktop uses `src-tauri/resources/native-agent-driver.mjs` to translate these
service events into its chat, tool activity and approval controls. Every conversation
has its own project root and history. `thread/start` with the same session ID resumes
saved history after a service restart. Knowledge remains exclusively behind `wiki mcp`;
the agent never opens the knowledge database.

## Build and run

Node 22.19+ (CI uses 24), Rust 1.88+:

```sh
npm run native:build -w athenaeum-agent
npm run native:check -w athenaeum-agent
npm run native:verify-source -w athenaeum-agent
node --test packages/agent/runtime/src/*.test.mjs
wiki agent -- --help
wiki agent -- -C /path/to/project
wiki agent -- exec --json -C /path/to/project "Investigate the failing tests"
```

The executable is `target/native-runtime/debug/athenaeum-agent` (`.exe` on Windows).
`ATHENAEUM_NATIVE_TARGET_DIR` overrides the build directory; `ATHENAEUM_AGENT_BIN`
selects an explicit executable. The manager supports `build`, `check`, `run` and
`install <bin-directory>`; install refuses to overwrite an existing executable.
`ATHENAEUM_NODE` selects Node for the platform launcher.

## Models and state

The desktop passes the chosen model, endpoint and key through its process environment.
For terminal use, create `~/.athenaeum-agent/config.json`, for example:

```json
{
  "model": "your-model-id",
  "api": "responses",
  "baseURL": "https://api.openai.com/v1",
  "apiKeyEnv": "OPENAI_API_KEY"
}
```

`api` supports `responses`, `chat-completions` and `anthropic`. Loopback HTTP endpoints
are supported for local models. Remote endpoints require HTTPS. Keys stay in the
selected environment variable. `ATHENAEUM_AGENT_HOME` selects another state directory.
`ATHENAEUM_AGENT_MODEL`, `ATHENAEUM_AGENT_BASE_URL`, `ATHENAEUM_AGENT_API` and
`ATHENAEUM_LLM_API_KEY` override model configuration for that process.
There is no subscription login or imported account/session migration.

Sessions are private JSON files under the agent home. `--session ID` resumes one;
`--ephemeral` does not retain a headless session. Project `AGENTS.md`, project
`.athenaeum/AGENT.md`, and the agent home's `AGENT.md` supply user instructions.
Skills are ordinary instruction files available through file tools; the runtime
has no independent plugin installer or hidden skill execution loop.

## Permissions and limits

Commands and file edits require explicit approval in the desktop or interactive
terminal. Noninteractive execution denies these actions unless the user supplies
`--yes`. Commands run with the user's privileges; **there is no OS sandbox**.
File tools constrain paths to the canonical workspace, reject symlink escapes and
private project state, and direct accepted wiki edits through proposals. Approved
shell commands can access resources beyond the workspace. Provider credentials
are removed from the shell environment. External MCP tools require approval;
wiki tools use the proposal-only server grant and never expose review operations.

A turn has at most 64 model steps, 32 tool calls per step and 6 MiB of conversation
context. Tool output is bounded to 64 KiB, text file reads/writes to 1 MiB, and images
to 5 MiB. Commands default to a two-minute timeout (maximum ten minutes). Cancellation
aborts model/MCP requests, pending approvals and running command groups. Context
limits produce an explicit error; history is never silently truncated.

This focused runtime does not claim the removed application's PTY terminal,
platform sandbox implementations, cloud services, voice, delegated agents,
automatic worktrees or context compaction. Git/worktrees and local development
tools are available through approved commands; additional integrations use MCP.

## Verification

`native-agent-config` covers configuration, persistence and negative permission/path
cases. `native-agent-smoke` exercises real patches/commands, the platform launcher,
and desktop approvals/denial/interruption against a scripted local provider. These
checks use no account or hosted model and do not establish live provider access.

This runtime is original Athenaeum code, GPL-3.0-only, under the root LICENSE.
