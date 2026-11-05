> Current source layout and root commands: [the consolidation guide](../../../docs/consolidation.md).
> Older sibling-checkout and pnpm development examples below are historical.

# Installing

For a person who wants to use the agent, not build it. Nothing needs to be installed first: the
installer brings its own Node and the exact harness release this bundle was verified against.

**Windows** (PowerShell, no administrator prompt):

```powershell
irm https://github.com/Hanoryx-North/athenaeum_agent/releases/latest/download/install.ps1 | iex
```

**macOS and Linux:**

```sh
curl -fsSL https://github.com/Hanoryx-North/athenaeum_agent/releases/latest/download/install.sh | sh
```

Then, in a project:

```sh
athenaeum-agent "why does the login flow validate state first?"
```

The command is the DeepSeek Harness's `dsh` with the `athenaeum` profile selected, so everything `dsh`
accepts works. It needs a model API key, as `dsh` always does (`DEEPSEEK_API_KEY`).

## What it installs

Everything goes under one directory, and nothing outside it is changed except a launcher link:

| Where | What |
| --- | --- |
| `…/Athenaeum/agent/node/` | A private Node, downloaded from nodejs.org and checked against a SHA-256 pinned in the script |
| `…/runtime/` | The DeepSeek Harness (`@deepseek-ai/dsh`) at the version in [`harness.pin.json`](../harness.pin.json), and the pnpm it shells out to |
| `…/dsh-home/` | The `athenaeum` profile, SDK JSON-RPC server and matching client/protocol libraries, plus harness state, kept apart from any `dsh` you already use |
| `…/bin/` | `athenaeum-agent` (the launcher) and `athenaeum` (the command line tool the bundle talks to) |
| `…/install.json` | What was installed and where, for tools that need to find the launcher |

The directory is `%LOCALAPPDATA%\Athenaeum\agent` on Windows, `~/Library/Application Support/Athenaeum/agent`
on macOS and `~/.local/share/athenaeum/agent` on Linux. On macOS and Linux the launcher and the CLI are
linked into `~/.local/bin`; on Windows `bin` is added to your user `PATH`. Open a new terminal afterwards.

The private Node is deliberate: the harness is installed against it, so the agent keeps working whatever
you later do to your own Node.

## Updating

Run the installer again. It reuses what is already in place, replaces the bundle and the CLI with the newest
release, and takes seconds. It does not touch your knowledge bases.

## Options

Pass them to the shell installer after `sh -s --`, or set the environment variable on Windows:

| Shell option | Windows variable | |
| --- | --- | --- |
| `--prefix DIR` | `ATHENAEUM_AGENT_PREFIX` | Install somewhere else |
| `--tarball PATH\|URL` | `ATHENAEUM_AGENT_TARBALL` | Install this bundle build instead of the latest release |
| `--cli PATH\|URL` | `ATHENAEUM_AGENT_CLI` | Install this `athenaeum` CLI archive instead of the latest release |
| `--no-link` | | Do not link the launcher into `~/.local/bin` |

## Removing

```sh
node install/install.mjs --uninstall            # or --prefix DIR for a non-default location
```

It removes only a directory that contains the `install.json` an install writes, and the two links it made.

## Requirements

- Windows 10 or later, macOS 11 or later, or Linux (glibc), on x64 or arm64. Windows on ARM runs the x64
  `athenaeum` build under emulation.
- Access to github.com and nodejs.org during install, and the npm registry for the harness.
- **The two repositories' releases must be readable without signing in.** The installer fetches the bundle from
  this repository's latest release and the CLI from Athenaeum's. While either repository is private, those
  downloads fail with a 404 and the installer says so; pass `--tarball` and `--cli` to install from files.

## What is verified, and how

Every push runs the real installers: `install.sh` on Linux and macOS, and `install.ps1` under Windows
PowerShell 5.1 on Windows, each into a path containing a space, from a clean runner. Each installed agent is
then started and asked to initialize the way Athenaeum's chat panel does ([`install/probe.mjs`](../install/probe.mjs)),
and the installer is re-run over the result to check the update path. See [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml).

## Desktop chat

Athenaeum reads `install.json`, starts its small Node driver with the private Node runtime,
and the driver imports the exact `@deepseek-ai/dsh-sdk-client` package installed in this
profile. The SDK launches the Harness runtime with the selected project as its workspace.
The app reports session and tool events; the SDK has no per-turn cancel method, so Stop
closes the runtime. The installer probe initializes this same public SDK against the real
profile without making a model request.

The installed profile enables the bundle's wiki curation tools, which make proposals only. The Harness profile's ordinary file
tools and permissions can still allow direct edits to `wiki/`; see the documented
[vault write boundary](vault-seam.md) for the current limitation.

## Remaining

- **The agent does not update itself.** The desktop app does; the agent is updated by re-running the installer.
- **No code signing.** The scripts are fetched over HTTPS from a GitHub release and the Node download is
  checksum-pinned, but the scripts themselves are not signed.
