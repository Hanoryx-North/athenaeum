# One Athenaeum checkout

Athenaeum's desktop/CLI, agent adapter, protocol, integrations and validation are
maintained in this repository. This replaces the sibling-repository layout.
`docs/consolidation.json` records the imported source revisions. The separate
`athenaeum_core` checkout is preserved for later work.

The import keeps the active TypeScript agent adapter and the native desktop's
app-server client. It excludes the retired Rust agent, old source-manifest
snapshots, nested Git metadata and generated build trees. Tests previously stored
as inline fragments are restored to their production modules; external test files
are restored beside their owners. The agent wire types are immutable aliases of the shared protocol, rather than
a second declaration of the same request/response schema. The agent reads the app's generated bridge
fixtures directly instead of maintaining another copy. Protocol fixtures and the
app's generated round-trip fixtures have different purposes; regenerate the
latter with `ATHENAEUM_BIN=... python3 validation/generate_app_fixtures.py`.

Run `npm ci` at the root. There is one npm workspace lockfile and one Cargo
workspace lockfile. `npm run build:packages` builds the protocol before its
consumers. `wiki dev --root /path/to/athenaeum components --json` describes actual
internal component paths. Running from the old parent directory also discovers
the app checkout, but no sibling project is required.

The MCP integration lives in `packages/integrations`; `npm run verify -w
athenaeum-integrations` builds the same standalone file used by the installer.
The optional Harness adapter lives in `packages/agent` and uses registry-pinned
upstream packages. Full Harness runtime tests require a separately built upstream
checkout through `DSH_REPO`; ordinary adapter tests do not. The desktop's native
runtime can be selected with `ATHENAEUM_AGENT_BIN`, or installed under the normal
agent prefix. The latest fetched agent revision supplies the native source under
`packages/agent/runtime`; build it with `npm run native:build -w athenaeum-agent`.
Its isolated Cargo manifest avoids unrelated upstream programs and preserves the
recorded source hash manifest. The binary lives in `target/native-runtime/debug`.
It does not invoke an installed Codex or share its credentials/state.

Hosted API, Library and admin applications are excluded from the local source
workspace. The local Library client and portable contracts remain for connecting
to a deployed service. Existing hosted services are not changed by this local
consolidation. The native runtime is included in platform CLI archives; the optional Harness
launcher is named `athenaeum-harness` so it cannot replace the native agent.
Historical hosted tests and retired sources remain recoverable in
the external backup, rather than being advertised as working local test steps.

Validation runs current sources directly. `python3 validation/verify_all.py --list`
shows the available checks; `--fail-on-skip` makes missing prerequisites or skipped
tests fail the run. Native desktop, installer and upstream runtime checks have
explicit platform/resource requirements and are opt-in. Reports describe exactly
which checks ran.

The [verification record](consolidation-verification.json) lists the completed local
checks and explicit release/platform exclusions. The existing frozen standalone
snapshot was not refreshed by this source consolidation; use live source development
or deliberately refresh it when needed.
