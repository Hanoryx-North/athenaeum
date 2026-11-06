> Current ownership: [AGENTS.md](../AGENTS.md) and the
> [consolidation guide](../docs/consolidation.md). Native runtime and adapters now
> live in `packages/agent`; MCP lives in `packages/integrations`. The UI remains
> an app-server client, with one knowledge engine behind its bridge.

# Frontend

Ported from [llm_wiki](https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su, and
modified for Athenaeum. **Every file in this directory carries that origin** unless its own
header says otherwise; see [`../docs/provenance.md`](../docs/provenance.md) and
[`../docs/porting-llm-wiki.md`](../docs/porting-llm-wiki.md).

Files with individual headers were substantially rewritten rather than copied.

## What is deliberately absent

The agent runtime lives under `packages/agent/runtime`, outside the frontend.
The desktop owns chat presentation; the runtime owns model turns, tools, permissions
and sessions. `lib/llm-client.ts` serves direct HTTP calls for ingest, not agent turns.
The optional Harness driver remains an external runtime adapter.

## Layout

```text
components/
  layout/     the shell — icon sidebar, resizable panels, knowledge and file trees
  editor/     wiki editor and reader, frontmatter, page links, history
  graph/      graph view over the knowledge index
  chat/       chat UI — native agent events, tools and approvals
  sources/ search/ lint/ review/ project/ settings/
  ui/         primitives
lib/          vault I/O, ingest pipeline, wikilinks, dedup, embeddings, research
  knowledge-api.ts   Athenaeum's own binding to the knowledge library commands
stores/       zustand state
hooks/  i18n/  types/  test-helpers/
```
