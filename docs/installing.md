# Installing Athenaeum

Use the platform installer from the GitHub release. It supplies the application,
CLI, MCP integration and private Node runtime. The built-in coding assistant is
`athenaeum-agent`; the optional separately installed Harness launcher is
`athenaeum-harness` and cannot overwrite it.

Open or create a project. Select a model provider in Settings to use app chat.
Under **Connectors → Coding agents**, choose **Athenaeum Agent** to connect the
project's wiki memory, or choose an optional Claude Code/Cursor MCP connection.
Setup verifies the connection before showing Ready. Wiki memory and model access
are separate: local knowledge needs no API key; the assistant uses your configured
model provider. See [agent memory](agent-memory.md).

## Command-line installation

The release's `install/install.mjs` supports a selected prefix and external agent
connections. Platform `install.sh` / `install.ps1` wrappers run the same installer.
Supported options include `--prefix DIR`, `--agents athenaeum,claude,cursor`,
`--capture off|instruct|nudge`, `--auto-sync`, and `--no-agents`. Read `--help`
for the release's full options. Existing user configuration is preserved; managed
blocks are backed up, and malformed or manually owned entries are left untouched.

`wiki setup` creates or connects a wiki; `wiki doctor` verifies local wiring;
`wiki disconnect-agents` removes only managed integration entries. Saved model
credentials and unrelated configuration remain yours. Auto sync is opt-in.

## Agent state and models

The built-in assistant stores `config.json`, `AGENT.md` and private conversation
files under `~/.athenaeum-agent` (`ATHENAEUM_AGENT_HOME` overrides it). Configure
terminal models there; the desktop passes its Settings route through the child
process environment. Keys belong in environment variables, not source files or
wiki pages. The runtime supports Responses, Chat Completions and Anthropic APIs.
Local model servers may use loopback HTTP; remote endpoints require HTTPS.

Commands and file changes require approval. Headless commands deny changes by
default; `--yes` is an explicit opt-in. Approved commands run with user privileges,
without an OS sandbox. See [runtime limits](../packages/agent/runtime/README.md).

External clients retain their own accounts and execution. Claude configuration
honours `CLAUDE_CONFIG_DIR`; Cursor uses its MCP settings. No external agent
credentials or sessions are imported into Athenaeum.

## Source builds and verification

Use the [README](../README.md) for the supported build commands. Build workspace
packages before consumers and run the validation runner with `--fail-on-skip`.
The native agent build needs only Rust and Node; it does not compile a copied
agent workspace. Platform desktop installers need the platform's Tauri tooling.
Builds and scripted local tests do not prove hosted provider access or validate
installers on another operating system.
