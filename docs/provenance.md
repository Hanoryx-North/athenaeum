# Provenance of copied code

This project was bootstrapped from two existing codebases rather than written
from scratch. This file records exactly what came from where, so that fixes can
be carried across in either direction and nobody has to guess later.

## From `llm_wiki`

| Here | Upstream | Change |
| --- | --- | --- |
| `crates/athenaeum-providers/src/client.rs` | `src-tauri/src/agent/provider.rs` | Two import lines. Nothing else. |
| `crates/athenaeum-providers/src/proxy.rs` | `src-tauri/src/proxy.rs` | Module doc only, including a note that only `configure_http_client` has a caller here yet. |
| `src-tauri/src/panic_guard.rs` | `src-tauri/src/panic_guard.rs` | Kept the synchronous half; dropped the async half and its `futures` dependency, since every command here is synchronous. |
| `src-tauri/windows-app-manifest.xml` | same path | Verbatim. |
| `src-tauri/build.rs` | same path | Verbatim, plus a comment on why the manifest matters. |
| `.github/workflows/*.yml` | `.github/workflows/*.yml` | Structure and the platform matrix. Rewritten for this project: no PDFium, no MCP server, no browser extension; workspace-root cache and bundle paths; added format, clippy, CLI smoke and icon-reproducibility jobs. |
| `tauri.*.conf.json` layout | same paths | The idea of per-platform overlay configs. Contents are this project's. |

`client.rs` and `proxy.rs` are the two files worth being careful with. Both
were already rustfmt-clean, so running `cargo fmt` across this repo did not
touch them — a diff against upstream still shows only the intended changes.
Prefer upstreaming a fix to `llm_wiki` over diverging here.

What was **not** taken: the document-centric parts (PDFium, ebook and office
parsing, LanceDB vector store, the MCP server, the browser extension) and the
`agent/` runtime. The runtime is the interesting one — it is a working agent
loop, and it is likely the right starting point for Phase 1 — but it is built
around llm_wiki's document model rather than a knowledge graph, so porting it
is real work rather than a copy.

## From `claude-code-source-code`

Nothing was copied. It is a TypeScript/Bun CLI built on Ink, and this is a Rust
desktop application; there is no file that transfers directly.

It was used as a **reference** for two things:

- The shape of a tool contract — name, JSON-schema input, permission check,
  rendered result — which is what Phase 1's tool system should look like.
- The separation between a tool's implementation and its prompt text.

When the agent loop is built, `src/Tool.ts`, `src/tools/*/prompt.ts` and
`src/services/tools/toolOrchestration.ts` are the files to read first.

## Generated, not copied

`src-tauri/icons/*` are produced by `scripts/generate_icons.py`. They are
placeholders and share nothing with llm_wiki's branding. CI regenerates them
and fails if the committed files differ.
