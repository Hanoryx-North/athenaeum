# Frontend

Ported from [llm_wiki](https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su, and
modified for Athenaeum. **Every file in this directory carries that origin** unless its own
header says otherwise; see [`../docs/provenance.md`](../docs/provenance.md) and
[`../docs/porting-llm-wiki.md`](../docs/porting-llm-wiki.md).

Files with individual headers were substantially rewritten rather than copied.

## What is deliberately absent

The agent runtime. llm_wiki drove chat through an in-process Rust agent and through
external `claude` / `codex` CLI subprocesses. Athenaeum does neither: the agent loop,
its tools, permissions and sessions belong to DeepSeek Harness, reached through
`athenaeum_agent`. `lib/llm-client.ts` therefore refuses the `claude-code` and `codex-cli`
providers and serves only the direct model calls ingest needs.

## Layout

```text
components/
  layout/     the shell — icon sidebar, resizable panels, knowledge and file trees
  editor/     wiki editor and reader, frontmatter, page links, history
  graph/      graph view over the knowledge index
  chat/       chat UI — its transport is being rewired onto athenaeum_agent
  sources/ search/ lint/ review/ project/ settings/
  ui/         primitives
lib/          vault I/O, ingest pipeline, wikilinks, dedup, embeddings, research
  knowledge-api.ts   Athenaeum's own binding to the knowledge library commands
stores/       zustand state
hooks/  i18n/  types/  test-helpers/
```
