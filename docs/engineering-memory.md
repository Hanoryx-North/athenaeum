# Engineering memory

Athenaeum keeps reviewed guidance in its existing wiki. Core memories can seed a
task brief without a keyword match; ordinary memories still use task search.
Both remain reference material, never permissions or replacements for project instructions.

`wiki principles --json` lists the ten starter principles distilled from the
owner's engineering and agent CLI talks. `wiki principles --propose` queues them
for review in the current wiki; repeating it skips existing titles, including
rejected proposals. It does not accept them or enable automatic saving.

## Match guidance to the work

Memories can carry optional `applicability` with `core`, `projects`, `technologies`
and `stages`. Stages are `initialize`, `design`, `coding`, `debugging`,
`verification` and `tools`. Empty selector lists match any value; selectors within
a list are alternatives, and all populated lists must match. Unknown project,
technology or stage excludes memories restricted by that selector.

```sh
wiki brief "Start the app" --stage initialize --project-id my-app --technology react --max-nodes 8 --json
wiki create --type procedure "Check render boundaries" --summary "Check imports before changing the renderer." --applicability '{"core":true,"projects":["my-app"],"technologies":["react"],"stages":["coding"]}'
```

The bridge `context` request and `wiki_context` tool take `selection`:
`{"project":"my-app","technologies":["react"],"stage":"coding"}`.
The bridge `create` request and `wiki_create` tool accept `applicability`.
Filtering precedes seed limits and graph expansion. Task matches seed the brief
before core guidance. Core guidance enters recall after it is saved. Acceptance
preserves selectors in Markdown frontmatter and rebuilding restores them.
Older memories without selectors keep their previous applicability; they are
not inferred to belong to another project.

The native agent supplies its normalized workspace path as the default project
identity, detects common manifest technologies, and selects a stage from the task.
An empty directory starts at `initialize`. These are deterministic heuristics,
not semantic classification. `memoryContext` in the agent's `config.json` can set
an explicit `project`, `technologies` and `stage`. The stage is optional; omit it
for automatic selection. Use a stable project identity across machines when
needed. Recall includes selectors only when the MCP tool advertises support.

## Add lessons

Use **Add knowledge → Paste text** with the configured memory model. Athenaeum
splits the text into scoped lessons, cites the submitted source and proposes
supported relationships to existing and newly extracted knowledge. Selectors are
chosen per lesson; unclear applicability stays local to the wiki. Saving a rule
does not install a lint rule or a project template.

CLI equivalent: `wiki desktop invoke background_memory --stdin --json` with
`{"projectPath":"/wiki","action":"ingest","document":{"text":"...","wikiRoot":"/wiki"}}`.
CLI callers can optionally set `title`, override `scope` (`project` or `universal`),
or mark the result `core` guidance.
Inputs are limited to 64 KiB and processed in bounded chunks. Interrupted work
uses chunk checkpoints; a crash during proposal creation can leave partial side
effects, so inspect failed jobs before resubmitting. This uses the separate
memory model; tests use controlled responses, not a live extraction-quality benchmark.

## Enforcement and verification

`npm run check:boundaries` checks product source imports and engine dependencies
in CI. It keeps runtime implementations separate and directs consumers through
the protocol and bridge. It does not prove all architectural properties.

Native `read_file` supports zero-based line `offset` and `limit` (default 200,
maximum 2000), returning `nextOffset` when more lines exist. Commands preserve
exit status, timeout, a bounded beginning/end excerpt, and private `logPath`
output when run by the native agent. Logs are capped at 64 MiB with explicit
`logTruncated`; they remain outside wiki sync. Unix commands use Bash with
`pipefail`; Windows retains cmd.exe pipeline behavior. Oversized results never
clip command status or produce partial JSON; other oversized tool results ask
for a smaller range instead of cutting a knowledge trust wrapper.

Regression coverage: `crates/athenaeum-core/tests/applicability.rs`, protocol
fixtures, `packages/agent/runtime/src/*.test.mjs`, and the product validation suites.
