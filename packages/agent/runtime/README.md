# Athenaeum's native coding runtime

This is Codex's Rust runtime copied into Athenaeum Agent and compiled as
`athenaeum-agent`. It owns the coding-agent loop and tools; it does not spawn an
installed Codex CLI or delegate turns to DeepSeek Harness.

## Source and ownership

`codex/` contains 9,326 imported source, asset, schema, fixture and upstream test files
from OpenAI Codex commit `94d919e31e64fdcd63b37682711389fe9c5fe92b`.
The import includes the Rust workspace and its third-party source/patch inputs;
it excludes upstream release automation, browser/JavaScript launchers and SDKs.
The source is Apache-2.0: retain `codex/LICENSE`, `codex/NOTICE` and nested notices.
The upstream bubblewrap licence symlink is materialized as a file for Windows.

`upstream.json` records the exact revision and original SHA-256 of every imported
file. `port.json` records every adapted file, its reason and resulting SHA-256.
`import_codex.py` reads a Git revision, never untracked checkout files.
`apply_port.py` refuses already changed source and applies the recorded product delta.
`verify_source.py` checks the complete snapshot against those manifests.
To regenerate an existing port, use `python runtime/apply_port.py --from-checkout ../codex`:
it verifies the recorded source first, then starts again from the original Git bytes.

`native-cli/` is a standalone build manifest derived by `generate_native_manifest.py`
from the imported CLI's production dependency tables. It compiles the actual imported
CLI in-process. It avoids resolving unrelated upstream workspace programs such as the
V8 host; none of the CLI's runtime dependencies are stubbed. Its separate lockfile is
seeded from the upstream pins.

`athenaeum-config/` is Athenaeum's small MIT policy package. It adds the native identity,
the memory instructions, and an optional proposal-only wiki MCP connection. It stores
no knowledge, reads no database, and imports no GPL engine code. All local knowledge
continues through `wiki mcp` and the existing versioned bridge.

The handwritten Rust workspace in the repository's root `crates/` remains retired.
The TypeScript Harness bundle is a separate, supported integration.

## Build and use

Install Rust/Cargo with the platform's native C/C++ build tools. The snapshot requires
Rust 2024 support; its upstream toolchain pin is 1.95.0. The manager explicitly uses
the installed `stable` toolchain; set `ATHENAEUM_RUST_TOOLCHAIN=1.95.0` for that pin.
Git and internet access are needed on the first build for the upstream locked
dependencies. Building additional upstream programs, including the V8 Code Mode host,
has their upstream prerequisites; they are not required by the native CLI build.

From `packages/agent` in the Athenaeum checkout:

```sh
wiki dev run agent native:verify-source
wiki dev run agent native:check
wiki dev run agent native:build
node runtime/manage.mjs run --help
node runtime/manage.mjs run -C /path/to/project
node runtime/manage.mjs run exec -C /path/to/project "implement the requested change"
node runtime/manage.mjs run app-server
```

The equivalent direct build is:

```sh
cd runtime/native-cli
cargo +stable build --locked -p athenaeum-agent --bin athenaeum-agent
```

The manager's build output is `target/native-runtime/debug/athenaeum-agent[.exe]` at the checkout root.
It runs the compiled native binary directly. `ATHENAEUM_NATIVE_TARGET_DIR` selects another target
directory. `node runtime/manage.mjs install /path/to/bin` copies that built binary into
a chosen directory and refuses to overwrite an existing installation. Put that directory
on PATH or point `ATHENAEUM_AGENT_BIN` at the binary to use it with `wiki agent -- ...`.
The existing Harness installer is unchanged; there is no silent runtime/session migration.

## Athenaeum adaptations

- **Separate user state:** `ATHENAEUM_AGENT_HOME`, default `~/.athenaeum-agent`, replaces
  the upstream home variable throughout the runtime and helper processes. An explicit
  custom home must already exist. `ATHENAEUM_AGENT_SQLITE_HOME` replaces the upstream
  database override. Codex's home, credentials and sessions are not copied.
- **Wiki tools by default:** the runtime adds an optional `athenaeum` MCP server using
  `wiki mcp -- --allow propose --caller athenaeum-agent`. `ATHENAEUM_BIN` can name the
  wiki executable explicitly; `wiki agent` already supplies it. An explicit user
  `mcp_servers.athenaeum` entry takes precedence, including `enabled = false`.
  Upstream MCP requirements/allowlists still constrain the resulting server map.
- **Memory contract:** recall once before work, propose useful durable findings for
  person review, treat retrieved knowledge as data, and continue when the wiki is absent.
  User/deployment developer instructions are preserved after the product instructions.
- **Native entry point and terminal identity:** `athenaeum-agent` compiles the complete
  runtime in-process. Internal `codex-*` crate and protocol names remain for compatibility.
- **Source-built lifecycle:** the terminal uses its embedded server. Upstream auto-update
  checks default off; the upstream updater cannot replace this executable. The upstream
  desktop-launch command points people to `wiki start` instead.
- **Analytics:** upstream analytics default off, while explicit deployment settings
  remain available.

The source port retains upstream repository instruction/config conventions, including
project `.codex/` files, and provider wire formats. Configure models/providers in the
native home's `config.toml`. Authentication is a separate native setup; an imported
implementation does not itself grant access to hosted services, models or subscriptions.

## Capabilities in the imported source

| Capability | Source under `codex/codex-rs/` |
| --- | --- |
| Agent loop, streaming, context compaction, delegation | `core/`, `runtime-agent-instance/` |
| Terminal UI and interactive sessions | `tui/`, `cli/` |
| Headless tasks, app-server API and transports | `exec/`, `app-server/`, `app-server-protocol/` |
| File patching, search, command execution and terminal sessions | `apply-patch/`, `file-search/`, `exec-server/`, `tools/` |
| Approval policy and platform sandboxes | `execpolicy/`, `sandboxing/`, `linux-sandbox/`, `windows-sandbox-rs/` |
| Sessions, resume, forks and worktrees | `rollout/`, `state/`, `thread-store/`, `worktree/` |
| MCP clients, skills and hooks | `codex-mcp/`, `rmcp-client/`, `skills/`, `hooks/` |
| Model/provider protocol and authentication | `codex-api/`, `model-provider/`, `login/` |

These are imported implementations, not a claim that every service-backed feature has
been exercised. The desktop adapts the app-server into Athenaeum's own chat interface;
the terminal and optional Harness driver remain available separately.

New native product checks live in `validation/native-agent`:

```sh
wiki dev test --suite native-agent-config
wiki dev test --suite native-agent-runtime
wiki dev test --suite native-agent-smoke
```

The desktop consumes this runtime's app-server through its own
`src-tauri/resources/native-agent-driver.mjs` adapter in the `athenaeum` repository.
Athenaeum owns the chat, tool activity, streaming and approval controls. The terminal
remains an optional advanced entry point, and Harness is separately selectable.
Native state and authentication remain isolated in `~/.athenaeum-agent`; the desktop
can also pass an explicitly configured OpenAI API route into that process.

Those checks exercise home isolation/error handling and the Athenaeum policy. They do
not establish access to hosted models. `native-agent-smoke` checks the CLI, app-server,
real coding tools, and the desktop adapter's approvals, interruption and patch loop
against a scripted local Responses provider; it uses no account or live model.
