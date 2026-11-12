# Athenaeum Agent runtime

Athenaeum's own coding assistant, maintained here as Node modules and a small
Rust platform launcher. It is a feature of the Markdown wiki application. It has
no copied coding application, terminal UI framework, cloud task service, voice
stack or upstream agent build graph. The launcher embeds the modules, extracts
them into a private temporary directory, runs Node, and removes that directory
on normal exit. Packaged desktops supply their private Node executable.

## Ownership

| Source | Responsibility |
| --- | --- |
| `src/agent.mjs` | Model/tool turn loop, project instructions and conversation state |
| `src/context.mjs` | Bounded conversation summaries and recoverable checkpoints |
| `src/learning.mjs` | Separate bounded learning jobs, source checks and proposal capture |
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
wiki agent -- inspect --json -C /path/to/project --session SESSION_ID
wiki agent -- memory
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

Before each turn, the runtime quietly calls the connected wiki's `wiki_context`
once using the task text. It supplies the engine's reference-material wrapper
verbatim to the coding model without adding chat or tool activity messages.
Empty, unavailable or oversized results do not interrupt coding or trigger retries.
The connection's `capture: "off"` setting disables this automatic lookup. Explicit
wiki tools remain available. No extra model request is needed for recall.

`inspect` reads saved memory diagnostics and compaction records without returning
conversation contents or making a model call. The app-server exposes the same
information through `thread/read` after `thread/start` or `thread/resume`.

## Background learning

Background extraction is opt-in because it spends additional model usage. Configure
an inexpensive model supported by the selected connection in the agent's `config.json`:

```json
{
  "backgroundMemory": {
    "enabled": true,
    "model": "your-supported-inexpensive-model"
  }
}
```

By default, the background model uses the same endpoint, API and credentials as coding.
Desktop Background memory can instead select `backend: "chatgpt"`, using the official
Codex app-server's saved ChatGPT connection. Credentials remain in that runtime.
Neither route falls back to the coding model. No job runs without
an explicit model and enabled setting. `capture: "off"` also disables learning.

A completed coding turn queues a private bounded job. A separate worker in the
native service processes jobs without chat messages or tool activity. Terminal
execution prints the coding result before draining learning work. Ephemeral turns
retain no learning jobs. Pending jobs resume after a restart; failed jobs remain
diagnostic records and are not automatically retried.

Each job makes at most one background model request, limited to 2048 output tokens,
and proposes at most two
findings. Turns without successful source observations make no extraction request.
API extraction gets no tools. ChatGPT extraction uses an ephemeral read-only thread,
disables shell and configured MCP tools, and rejects tool attempts. The worker checks exact excerpts
against current files observed in the turn, rejects private paths and common
credential formats, and searches existing knowledge before creating a proposal.
Only new proposals receive evidence. Nothing is accepted or written to the vault.
Source matching supports review; it does not establish that a model's interpretation
is correct. Completed jobs retain diagnostics without the turn transcript.

`wiki agent -- memory` and app-server `memory/read` expose job status without
transcripts. New jobs are deferred when eight jobs already await processing.
The model receives a completed result and numbered observed source excerpts, then
selects references for the worker to attach. Cross-workspace excerpts use external
evidence rather than fictitious paths inside the memory wiki. The desktop starts
`memory-server` independently of chat; it polls completed local Codex turns through
`thread/list` and `thread/turns/list`, keeps a cursor and ignores reasoning/images.
Resuming after a pause does not import turns from the paused interval.
Each job has a 128 KiB input limit. An OS-owned process lock prevents concurrent
workers from processing the same queue, and releases automatically on process exit.

## Conversation continuation

The default input budget is 256 KiB, counting history, system instructions and tool
schemas. Set `contextMaxBytes` in the agent's `config.json` to change this budget,
between 16 KiB and 6 MiB. This is a byte guard, not a model token-window measurement.
Choose a smaller budget for models with smaller context windows.

At 75 percent of that budget, the runtime summarizes completed history using the
selected coding connection. This makes an additional model request on that connection,
limited to 4096 output tokens or the configured `maxOutputTokens`, whichever is lower.
It retains the latest user request, recent history, and complete tool call/output
groups. Summaries are reference material and never grant approval. The original
history is archived under `sessions/SESSION_ID.checkpoints/` before the replacement
is saved atomically. Interrupted or failed summaries leave the conversation intact.
Checkpoints stay private to the agent home and are not knowledge or sync exports.

Set `autoCompact: false` to disable summarization. Oversized individual inputs or
tool groups fail explicitly instead of being discarded. Summary text does not enter
chat; context diagnostics remain available through `inspect` and `thread/read`.
This uses the local summary/checkpoint pattern studied in OpenAI Codex's
`codex-rs/core/src/compact.rs`. No upstream source was copied.

## Permissions and limits

Commands and file edits require explicit approval in the desktop or interactive
terminal. Noninteractive execution denies these actions unless the user supplies
`--yes`. Commands run with the user's privileges; **there is no OS sandbox**.
File tools constrain paths to the canonical workspace, reject symlink escapes and
private project state, and direct accepted wiki edits through proposals. Approved
shell commands can access resources beyond the workspace. Provider credentials
are removed from the shell environment. External MCP tools require approval;
wiki tools use the proposal-only server grant and never expose review operations.

A turn has at most 64 coding model steps and 32 tool calls per step.
Tool output is bounded to 64 KiB, text file reads/writes to 1 MiB, and images
to 5 MiB. Commands default to a two-minute timeout (maximum ten minutes). Cancellation
aborts model/MCP requests, pending approvals and running command groups.
If interrupted within a tool group, the runtime checkpoints completed results and
marks unfinished operations explicitly. A started operation without a recorded
result requires inspecting workspace state before retrying.

This focused runtime does not claim the removed application's PTY terminal,
platform sandbox implementations, cloud services, voice, delegated agents,
automatic worktrees. Git/worktrees and local development
tools are available through approved commands; additional integrations use MCP.

## Verification

`native-agent-config` covers configuration, persistence and negative permission/path
cases. `native-agent-smoke` exercises real patches/commands, the platform launcher,
and desktop approvals/denial/interruption against a scripted local provider. These
checks use no account or hosted model and do not establish live provider access.

This runtime is original Athenaeum code, GPL-3.0-only, under the root LICENSE.
