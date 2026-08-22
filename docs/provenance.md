# Provenance of copied code

This project was bootstrapped from existing codebases rather than written from scratch.
This file records exactly what came from where, so that fixes can be carried across in
either direction and nobody has to guess later.

## Licence

**Athenaeum is GPL-3.0-only.** It derives from
[llm_wiki](https://github.com/nashsu/llm_wiki) — GPL-3.0, © Yong Su — whose UI and wiki
engine Athenaeum ports. See [`porting-llm-wiki.md`](porting-llm-wiki.md).

The repository was previously labelled MIT. That was already incorrect before the port
began: the files listed below were copied from llm_wiki at bootstrap, and GPL-3.0
obligations attached then. The manifests, `LICENSE` and this note now agree with what the
code actually is.

The obligation stops at Athenaeum's edge.
[`athenaeum_agent`](https://github.com/Hanoryx-North/athenaeum_agent) and
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) remain MIT: separate
repositories, separate processes, connected only by the `athenaeum bridge` protocol, which
is a data format rather than linkage. **Do not copy Athenaeum code into either of them.**

Substantially copied files carry a header naming their origin:

```
// Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
// Modified for Athenaeum.
```

## From `llm_wiki` — bootstrap

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

At bootstrap the document-centric parts (PDFium, ebook and office parsing, LanceDB vector
store, the MCP server, the browser extension) and the `agent/` runtime were left behind.

**Both of those decisions have since changed, in opposite directions.** The document
parsing and vector store are now being ported — they are the auto wiki builder, which is
Athenaeum's job. The `agent/` runtime is now permanently refused: DeepSeek Harness owns the
agent loop, and porting llm_wiki's 13,512-line runtime would mean maintaining a second one.
[`porting-llm-wiki.md`](porting-llm-wiki.md) records the full subsystem-by-subsystem
decision.

## From `neural-notes` (SiYuan)

**Nothing is copied, and nothing may be.** neural-notes is
[SiYuan](https://github.com/siyuan-note/siyuan) v3.8.1, AGPL-3.0, © Yunnan Liandi
Technology Co., Ltd. It is a Go kernel with an Electron/vanilla-TypeScript frontend built
on its own `.sy` block format — architecturally incompatible with this Tauri/React/Rust
application, and its AGPL-3.0 terms are stronger than the GPL-3.0 this project carries.

It is used as a **visual reference only**: layout density, chrome, spacing, overall calm.
No SiYuan code, CSS or assets enter this repository.

## From `claude-code-source-code`

Nothing was copied. It is a TypeScript/Bun CLI built on Ink, and this is a Rust
desktop application; there is no file that transfers directly.

It was used as a **reference** for two things:

- The shape of a tool contract — name, JSON-schema input, permission check,
  rendered result.
- The separation between a tool's implementation and its prompt text.

The agent loop is no longer anyone's port of it: the runtime is DeepSeek
Harness, which owns tools, permissions, sessions and providers as plugins.
Nothing of that belongs in this Wiki Library repository either way.

## Generated, not copied

`src-tauri/icons/*` are produced by `scripts/generate_icons.py`. They are
placeholders and share nothing with llm_wiki's branding. CI regenerates them
and fails if the committed files differ.
