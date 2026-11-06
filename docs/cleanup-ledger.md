# Codebase cleanup ledger

## Contract

Scope: audit the local application and replace the copied coding application with
an Athenaeum-owned runtime. Remove the retired product's branding, setup route,
CLI model presets and build dependencies. Preserve the wiki engine, versioned
bridge, review boundary, hosted sync formats and optional Harness integration.
The separate sibling engine checkout and existing user state are excluded.
Baseline: `52598e6`; working tree was clean. Git retains removed sources for rollback.
No accepted wiki pages or external agent configuration are edited by this campaign.

## Audit and plan

- [P1] `packages/agent/runtime/`: 112 MB / 9,326 imported runtime files, including
  unrelated cloud, terminal, voice and platform programs. Replace with focused
  owned runtime modules and a small executable launcher. Verify model/tool loop,
  path safety, approvals, cancellation, session persistence and desktop seam.
- [P1] Settings advertised CLI provider commands absent from the native command
  registry. Remove the retired provider and obsolete status/settings controls.
- [P2] Setup, release packaging, validation and docs assumed the copied runtime.
  Rewire these together; retain external Claude/Cursor MCP integrations.
- [P2] README described multiple incompatible layouts and start behavior.
  Reconcile with current CLI and one-repository ownership.

## Batches and verification matrix

1. Owned runtime + launcher: Node behavior/security tests, native build and
   scripted Responses provider/desktop approval integration tests.
2. Product identity and setup: connector tests, frontend tests/typecheck/build,
   integration bundle and CLI discovery/product tests.
3. Repository consistency: engine/protocol/package suites, native desktop check,
   tracked-source reference scan, diff/whitespace review and updated documentation.

## Baseline evidence

Package checks and frontend tests passed before edits; results are recorded below.
The installed `wiki` is older than this checkout and has no `desktop` subcommand;
use the source-built CLI for current command discovery.


## Completed batches

- B01: replaced the imported application tree and hash/import tooling with six
  owned Node modules and a dependency-free Rust launcher. Runtime source is about
  112 KiB including its tests/docs, versus the previous 112 MiB snapshot. Kept one
  model/tool loop outside the knowledge engine. Added file/search/patch/image and
  command tools, explicit approvals, cancellation, bounded execution and output,
  private sessions/resume, and proposal-only wiki MCP. Completed tool rounds are
  checkpointed before the next model request; command failures are reported as
  failed tool activity. Paths are canonicalized, private state and accepted wiki
  writes are blocked, and symlink edits/escapes are rejected. No runtime speedup
  or memory benchmark is claimed from the source-size reduction.
- B02: replaced the retired connector with the built-in agent's JSON configuration
  and managed instructions. Kept Claude/Cursor MCP connections. Removed inactive
  CLI model presets whose native transports were already absent, along with their
  dead status controls, isolation options and translations. Retired only tests for
  those explicitly removed controls; preserved the live engine/protocol tests and
  added owned runtime, route, permission and persistence coverage. Repaired model
  routing so supported Settings APIs reach the agent and unsupported routes fail
  clearly. Explicitly keyless routes cannot inherit another provider's API key.
- B03: rewired platform packaging, CLI/private Node discovery, installation status,
  native source checks, CI suite names and documentation. Integration builds clear
  generated output first so deleted connectors cannot ship in tarballs. Source and
  built app/integration bundles have no retired product-name references. Git history
  and user-authored knowledge/configuration were not rewritten. The obsolete
  consolidation verification snapshot was retired; current evidence is in
  `docs/cleanup-verification.json`.
- B04 (security dependency batch): compatible locked updates plus explicit PDF.js
  6.5.438 and Zod 4.6.7 upgrades. Updated PDF resource ownership to destroy the loading
  task under the new API; avoided creating a worker after unmount. Checked actual PDF
  parsing/text extraction/canvas rendering in a disposable browser. The full product
  suites verify schema/MCP behavior after the Zod/SDK update. npm audit decreased
  from 34 findings (including two critical and 15 high) to 14 low/moderate findings,
  with no high or critical findings. Advisory references:
  [PDF.js](https://github.com/advisories/GHSA-hq66-cqwq-w95j),
  [Zod](https://github.com/advisories/GHSA-mrhx-c7xc-m3w5).

## Verification completed

Baseline: package checks passed; 127 frontend files / 1,855 tests passed.
Final: all 20 default-plus-native-smoke/desktop suites passed with no skips:
`python3 validation/verify_all.py --with native-agent-smoke,app-desktop --fail-on-skip`.
This includes 128 frontend files / 1,844 tests, 125 optional-adapter tests, 14 owned
runtime tests, nine native/desktop smoke cases, and engine, CLI, contract, automation,
MCP, proposal/review/recall and negative boundary suites.

`npm run build`, native build/check, root and launcher formatting, launcher Clippy
with warnings denied, and `git diff --check` passed. `npm audit --audit-level=high`
passes. The setup browser fixture and PDF browser smoke pass. Setup's manual imports
require a fresh Vite module graph after editing stateful store modules; stale graph
instances were diagnosed and the server was restarted, without weakening assertions.
These browser fixtures simulate the Tauri boundary; they are not a native webview
visual inspection. The installed CLI predates this source; discovery used the
source-built `target/debug/wiki`. No desktop was running for live catalog calls.

## Remaining limits and ranked follow-up

1. The focused agent has explicit approvals but no OS sandbox, PTY terminal,
   delegated agents, cloud/voice stack or automatic context compaction. Approved
   commands run with user file/network privileges, disclosed in approval controls.
   Context limits produce an error rather than silently dropping conversation.
   Shell/MCP provide extension points; do not present this as complete capability
   parity with the removed application. New terminal use needs `config.json`;
   the removed application's state/configuration is not imported or deleted.
2. Fourteen npm advisory entries remain: ten low and four moderate, originating
   in KaTeX's ecosystem and shadcn build tooling (`deepmerge`, `minimatch` through
   ts-morph). Several reported fixes require unsupported downgrades/major overrides.
   Review dependency support and actual entry points before changing these together.
   No forced overrides or broad major updates were applied to hide the report.
3. Release installers, Windows/Linux execution, live hosted providers and the
   optional external Harness runtime were not exercised here. Existing dynamic
   import/bundle warnings remain; no measured performance claim is made for them.
   The frozen standalone snapshot and installed application were not refreshed.

Rollback is available through the baseline Git revision after preserving the current
working tree. There is no knowledge/schema migration. Existing credentials, sessions,
accepted Markdown and the separate sibling engine checkout were left alone.
