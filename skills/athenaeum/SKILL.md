---
name: athenaeum
description: Operate, inspect, develop, and troubleshoot Athenaeum through its wiki CLI and live desktop command interface. Use for Athenaeum files, knowledge nodes, relationships, proposals, tasks, connections, settings, accounts, sync, and live source changes. Does not replace ordinary coding tools or apply to unrelated apps.
---

# Athenaeum

Operate Athenaeum through CLI/API calls. Do not control the user's PC or click its
UI. If an operation lacks CLI support, add it to the owning implementation rather
than using computer control. Keep the knowledge engine authoritative.

## Discover the current commands

Use `wiki commands --json` for standalone commands, `wiki desktop actions --json`
for the running desktop, and `wiki dev tasks --json` for builds and checks.
Discovery is generated from the parser and app functions. Read the relevant
entry's parameters/signature before calling it. Do not invent command names.

Standalone operations work without the app. Live operations use
`wiki desktop call ACTION --stdin --json` with a positional JSON argument array,
or `wiki desktop invoke NATIVE_COMMAND --stdin --json` with a named JSON object.
On Windows PowerShell, stdin avoids native argument quote stripping.

```powershell
wiki desktop status --json
wiki desktop actions fs. --json
'["graph"]' | wiki desktop call wiki.setActiveView --stdin --json
wiki desktop call graph.snapshot --json
'["C:/path/to/wiki", {"maxDepth":3}]' | wiki desktop call fs.listDirectory --stdin --json
'["C:/path/to/wiki/wiki/overview.md"]' | wiki desktop call fs.readFile --stdin --json
wiki desktop state research --json
```

The live catalog includes native commands, domain services, state methods, and
registered surface actions. Methods such as `graph.snapshot` and `chat.send` are
available while their surface is mounted. Set `wiki.setActiveView` first, then
wait for the expected action to appear. Use `project.open` or `project.create`
for a full project switch; low-level `fs.openProject` only reads project metadata.

## Choose the right operation

- Knowledge: `list`, `show`, `search`, `navigate`, `path`, `brief`, `history`.
- Proposals: `create`, `proposals`, `accept`, `reject`. Follow the owner's saving
  policy. A model must not accept its own memory proposal.
- Relationships: `link`, `unlink EDGE_ID`, live `knowledge-api.api.linkNodes`, and the native
  relationship commands in discovery. Check persistence after an index rebuild.
- Files: live `fs.*` services for tree, read, write, existence, metadata,
  copy, deletion, project creation, and file history native commands.
- Tasks: live `deep-research.queueResearch`, `research.*`, `ingest-queue.*`,
  `dedup-queue.*`, `file-sync.*`, `review.*`, and `lint.*`.
- Agent connections: `setup`, `doctor`, `disconnect-agents`, or live `setup.*`.
- Accounts and hosted repositories: `library`, `sync`, `api`. Use stdin for
  credentials. Discovery documents the supported login flow.
- Saved settings: `config` with the app closed. Live state/settings:
  `desktop state`, `wiki.*`, and `project-store.*`. Store setters alone may be
  transient; use the matching save operation when persistence is required.
- App lifecycle: `desktop status`, `desktop show`, `desktop reload`, `desktop quit`.
  Platform operations are exposed as `system.*`, `window.*`, and `clipboard.*`.

Do not print connection descriptors or tokens. Live results redact secrets by
default. Use `--reveal` only when the user's task needs those values, keeping them
out of chat and logs. Timeouts do not prove that a mutation failed. Inspect state
before retrying.

## Keep source development live

In a source checkout, `wiki start` starts or reuses a managed live Vite/Tauri
session. `wiki start --dev` keeps it in the foreground. Logs are in the app
repository's `.athenaeum-dev/live.log`. Frontend TypeScript/CSS edits hot-update
the running webview. Confirm `wiki desktop status --json` reports `live: true`.
Rust changes need an incremental native build/restart; do not promise native
binary replacement without compilation. Never use a release build for routine UI
work. `wiki start --standalone` deliberately opens the frozen offline snapshot.

Verify the running result, not just a successful bundle. For a graph change,
`graph.snapshot` returns the actual Sigma graph and rendered node attributes.
Other surfaces expose live state and their domain operations. Do not claim a
visual screenshot inspection from a state-only check.

When developing a new feature, expose its domain operation through a cataloged
service/store action or `registerDesktopAction` next to its UI lifecycle. Keep
non-JSON callback APIs behind a JSON-compatible domain action. Run the relevant
`wiki dev check`, `wiki dev build`, and validation suites. Read the repository's
`AGENTS.md` and `docs/wiki-cli.md` for ownership and verification details.
