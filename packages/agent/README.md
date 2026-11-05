# Athenaeum Agent

This is the agent component of the [Athenaeum application](../../README.md).
The [root instructions](../../AGENTS.md) and [development guide](../../docs/consolidation.md)
define the current source layout.

The primary native runtime is in [`runtime/`](runtime/README.md). It compiles the
adapted upstream implementation in-process as `athenaeum-agent`, with its own home
(`ATHENAEUM_AGENT_HOME`, default `~/.athenaeum-agent`). It does not invoke an installed
Codex or copy its credentials/sessions. The snapshot, licences and every adaptation
are recorded and verified by its source manifests. The desktop uses its app-server
for turns, streaming, coding tools, sessions and approvals.

The optional TypeScript Harness adapter is in `src/`. It contributes the `wiki`
capability, bridge provider, `wiki_*` tools, task context and configured Wiki Agent
role. Harness remains an external, pinned dependency; the adapter contains no
second coding-agent loop or knowledge store. Immutable wire types come from the
shared protocol package. Agent fixtures are read directly from the app's generated
bridge fixtures, without another vendored copy.

From the repository root:

```sh
npm ci
npm run build:packages
npm run native:verify-source -w athenaeum-agent
npm run native:build -w athenaeum-agent
python3 validation/verify_all.py --only agent,native-agent-config,native-agent-smoke --fail-on-skip
```

The source binary is `target/native-runtime/debug/athenaeum-agent` (`.exe` on
Windows). `wiki agent -- ...` and the development desktop discover this build.
`ATHENAEUM_AGENT_BIN` explicitly selects another binary. Native runtime build
artifacts have their own target directory; `ATHENAEUM_NATIVE_TARGET_DIR` overrides
it. `wiki dev build agent` builds both the adapter and the native runtime.

All knowledge operations go through `wiki mcp` and the versioned bridge. Retrieved
content remains reference material, never instructions or permission. The runtime
can propose knowledge for the owner's saving policy; it never becomes another
knowledge engine.

The optional Harness profile/installer remain available. Source package dependencies
resolve through the root npm workspace and lockfile. The installed optional terminal
launcher is `athenaeum-harness`, so it cannot overwrite the primary native agent.
Full Harness runtime tests require `DSH_REPO` pointing at a built checkout of the
release in `harness.pin.json`; run the separate `harness-runtime` validation step.

Adapter details: [capabilities](docs/agent-layer.md), [bridge boundary](docs/integration-contract.md),
[profile mounting](docs/mounting.md). Native source details: [runtime/README.md](runtime/README.md).
