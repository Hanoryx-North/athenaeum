# Relay desktop builds

Root `relay.app.json` selects Athenaeum's Tauri desktop application, with exact
`macos-aarch64` and `macos-x86_64` targets. Relay installs everything under
`dist-relay/` and opens `Athenaeum.app`. This disposable directory is separate
from Vite's `dist/`, which the frontend build replaces.

The app requires macOS 13 or later, matching the pinned PDFium library's minimum
deployment version. The host needs Node 22.19+ (24 recommended), npm, Rust 1.88+
with its native Apple target, Xcode Command Line Tools, protobuf compiler
(`protoc`), and `tar`. The build does not install host toolchains.
Dependency downloads require access
to npm, crates.io, nodejs.org, and GitHub release assets. The root npm lockfile
and both Cargo lockfiles (the workspace and isolated native launcher) are used
unchanged.

Relay executes these steps from the repository root:

```sh
npm ci
npm run build:packages
npm run relay:build
```

`scripts/relay-build.mjs` bundles the MCP integration and optional MIT Harness
adapter, builds the CLI and owned native agent, and uses the existing release
packaging to collect their licenses and a checksum-pinned private Node runtime.
It downloads the existing checksum-pinned PDFium library and asks Tauri for only
the `.app` bundle. It copies the complete bundle into `RELAY_OUTPUT_DIR`, restores
runtime symlinks flattened by Tauri, and signs the final bundle. In particular,
Node's `bin/npm` must remain a relative symlink to its actual module entry;
copying that entry as a regular file breaks its relative imports. No installer,
app launch, agent setup, service, or publishing step runs. The optional Harness
runtime is still downloaded only when the user
selects that integration during setup.

These local builds use Cargo's incremental debug profile without release LTO.
Tauri still embeds the built frontend; no Vite server is required at runtime.
The app is ad-hoc signed. The generated overlay is ignored at
`src-tauri/tauri.relay.conf.json`; committed versions and release settings are
unchanged. Compiler outputs respect `CARGO_TARGET_DIR`, including the separate
launcher at `CARGO_TARGET_DIR/native-runtime`. Build staging under
`CARGO_TARGET_DIR/relay-staging` is disposable and does not ship.

For a manual Apple silicon check, start with an empty output folder and set
Relay's environment before running the three manifest steps:

```sh
export RELAY_PROJECT_DIR="$PWD"
export RELAY_OUTPUT_DIR="$PWD/dist-relay"
export RELAY_PLATFORM=macos
export RELAY_ARCH=aarch64
export CARGO_TARGET_DIR="$(mktemp -d -t athenaeum-relay)"
mkdir -p "$RELAY_OUTPUT_DIR"
npm ci
npm run build:packages
npm run relay:build
```

On an Intel Mac use `RELAY_ARCH=x86_64`. Use a new empty output folder when
repeating the check; Relay itself deletes and recreates the declared output.
Do not point the output at source or user data.

Validate structure without compiling:

```sh
relay-cli check-build /path/to/athenaeum --target macos-aarch64
relay-cli check-build /path/to/athenaeum --target macos-x86_64
```

Windows is not declared: the existing release path builds NSIS installers and
does not yet collect a portable app with its WebView2 dependency for Relay.
Linux is outside Relay's desktop contract.

## Verification

- Both targets pass the skill's JSON schema and Relay CLI `check-build`.
- All 35 release configuration and resource-symlink regression tests pass.
- Apple silicon packaging passed on 2026-11-17 with an empty output folder,
  a reduced environment, and a fresh Cargo output directory outside the checkout.
  Package type checks and the embedded frontend build passed.
- A copied bundle in a separate directory with spaces passed strict signature
  verification. Private Node 22.22.0, npm 10.9.4, CLI discovery, the native agent,
  and MCP integration ran with a system-only `PATH`. PDFium loaded and initialized;
  inspected native dependencies referenced only system or bundle-relative paths.
  All three runtime symlinks resolved within the copied app.
- Base install and update passed offline in an isolated temporary home using the
  relocated bundle's installer and resources. No GUI app was launched. These
  checks cover Athenaeum's bundled tools installer, separately from Relay itself.
- Intel compilation and execution require an Intel host and are untested here.
- Relay Install/Update from GitHub `main` remain untested until these changes are
  committed and available there. Contract checks do not establish installation
  or update behavior.
