# Engineering Rules

Production-quality, senior-engineer code: **correct, maintainable, modular, performant, and simple**. Optimize for correctness and long-term maintainability—not diff size, abstraction count, or feature count.

These rules govern a **Rust workspace with three consumers**: a knowledge library, a CLI, a Tauri desktop shell, and a deliberately thin React frontend inside it. Where a rule below is written for one of those surfaces it says so; the rest apply everywhere.

## 1. Before Coding

1. Understand the request and existing architecture. [`docs/architecture.md`](architecture.md) explains *why* the layering is what it is; read it before changing a layer. The `§N` references throughout the code point into [`docs/vision/build-plan.md`](vision/build-plan.md), the original specification.
2. Inspect relevant files, patterns, dependencies, and project rules.
3. Search before creating anything new.
4. Identify the **smallest correct change**.
5. Define verifiable success criteria.
6. Ask when requirements are genuinely ambiguous; never guess.
7. Verify framework/library behavior when uncertain.

Do not redesign stable architecture without a concrete reason.

## 2. Change Discipline

* Make surgical, behavior-preserving changes.
* Reuse existing types, helpers, store methods, and vocabularies.
* One source of truth; no duplicate implementations.
* Remove obsolete code created by the change.
* No speculative features, placeholders, invented APIs, URLs, data, or provider behavior.
* Do not restructure unrelated code.
* Every file has **one clear responsibility**.
* Do not churn code that already meets the standard.

### Three-Pass Rule

For meaningful work:

1. Implement.
2. Self-review and fix weaknesses.
3. Review again as a strict senior engineer.

Do not narrate the passes.

### Vendored Code

Files listed in [`docs/provenance.md`](provenance.md) were copied from another
codebase and are kept diff-clean against it on purpose.

```text
crates/athenaeum-providers/src/client.rs
crates/athenaeum-providers/src/proxy.rs
src-tauri/src/panic_guard.rs
src-tauri/windows-app-manifest.xml
src-tauri/build.rs
```

For these, **the upstream is the source of truth**:

* Fix bugs upstream and re-copy, rather than diverging here.
* Do not refactor them for style, duplication, or file size. The ability to
  diff them against upstream is worth more than the cleanup.
* Any deliberate divergence gets a row in `provenance.md` saying what and why.
* New code does **not** go in these files.

---

## 3. Architecture

The one decision everything follows from: **the knowledge library is a plain Rust library with no UI, no Tauri, no HTTP client and no model dependency.** Everything else consumes it.

```text
                    ┌──────────────────────┐
                    │   athenaeum-core     │   the product
                    │  model · store ·     │   no UI
                    │  graph · brief       │   no network
                    └──────────┬───────────┘
                               │
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
      athenaeum-cli      src-tauri + src/   Wiki protocol v1
                                                   │
                                                   ▼
                                      external agent repository
```

Dependencies point **inward only**:

| Crate / surface | may depend on |
| --- | --- |
| `athenaeum-core` | nothing in this workspace |
| `athenaeum-providers` | nothing in this workspace |
| `athenaeum-cli` | `athenaeum-core` |
| `src-tauri` | `athenaeum-core`, `athenaeum-providers` |
| `src/` (frontend) | the Tauri command surface, via `src/api.ts` |

Rules:

* **Nothing UI-, network-, or model-aware may enter `athenaeum-core`.** If a change wants a `reqwest`, `tauri` or provider import in that crate, the design is wrong, not the rule.
* **The agent runtime is a separate repository.** Do not add an agent loop,
  coding tools, shell execution, Git operations, streaming, runtime sessions
  or runtime permissions here. This repository implements the Wiki side of
  the contract documented in `docs/agent-integration.md`.
* A behavior reachable from the desktop app must be reachable from the CLI. That is what keeps the library testable without a window and provable on all three platforms in CI.
* Knowledge rules live in the library, never in a command handler or a React component. If a rule about what knowledge *means* appears outside `athenaeum-core`, move it in.

### Where a file goes

```text
crates/athenaeum-core/src/
  model/<thing>.rs       plain value types: no SQL, no I/O, constructible in a test
  model/enums.rs         closed vocabularies, via the `str_enum!` macro
  store/<aggregate>.rs   persistence for one aggregate (nodes, edges, evidence, events)
  store/schema.rs        table definitions and the append-only migration list
  graph.rs               traversal over the store
  brief.rs               assembling what an agent receives
  wiki.rs                typed, storage-independent agent integration boundary
  authority.rs           conflict resolution between claims
  workspace.rs           locating and creating `.athenaeum/`
  blobs.rs               content-addressed evidence material
  id.rs · error.rs       typed ids; the crate's error enum

crates/athenaeum-cli/src/
  main.rs                argument definitions and dispatch only
  bridge.rs              versioned Wiki JSON transport
  render.rs              turning library values into text or JSON
  check.rs               knowledge-base validation

src-tauri/src/
  commands.rs            one thin translation per command; no knowledge logic
  panic_guard.rs         the panic→error boundary (vendored)
  lib.rs                 plugin registration and the command table

src/
  App.tsx                composition only
  hooks/use<Thing>.ts    state and async orchestration
  components/<Name>.tsx  one component, props in, callbacks out
  api.ts                 the typed mirror of the Tauri command surface
  styles.css             the whole stylesheet
```

* One home per kind of thing. No second `components/`, no `features/`, no `utils/` grab-bag.
* `model/` holds values; `store/` holds persistence. A type that cannot be built without a database belongs in `store/`.
* A new `store/` submodule per aggregate, not per query.
* The frontend is small on purpose (see §9). Keep it flat: `components/<Name>.tsx`, not a folder per component.

---

## 4. The Knowledge Model

* A node is a **semantic object, never a text chunk**. `AuthService` is a node; "bytes 4000–4500 of AuthService.ts" is not. Source files stay the source of truth for implementation.
* **Provenance is never lost.** An edge derived from an `import` statement and one a model guessed are both edges, and the graph must always be able to tell them apart.
* **Nothing is true because a model said so.** New nodes default to `UnverifiedObservation`; new edges default to `Proposed` and are not traversed until promoted. Deterministic extraction arrives `Valid`.
* **Staleness is not falsehood.** Status and authority are separate axes and must stay that way.
* **Superseding beats deleting.** History is usually the valuable part. Retain and stop offering; do not remove.
* **Reading knowledge never rewrites it.** Retrieval, use and citation counts go to the journal and are derived on demand.
* Vocabularies are closed enums declared with `str_enum!`, so the on-disk spelling of a variant exists in exactly one place. Adding a variant means adding it there — never a second hand-written string table.
* `EdgeType::inverse()` must stay involutive. A test enforces this; do not weaken it.

---

## 5. Storage & Schema

Everything persists to one SQLite file plus a directory of content-addressed blobs. No server, no index build, no network to open a project.

* **Migrations are append-only.** Index `n` in `MIGRATIONS` migrates version `n` to `n+1`. Never edit an entry that has shipped; add one and bump `SCHEMA_VERSION`.
* **A database written by a newer build is refused, never downgraded.** Dropping columns this build does not understand destroys the user's knowledge.
* **A write and its journal entry share one transaction.** Either both land or neither does. Pass `&Connection` to `record_in` rather than taking `&mut self` twice.
* **Every mutation journals.** If a store method changes the graph and writes no event, that is a bug, not an omission.
* **Parameterized SQL only.** Bind values; never interpolate them, including numeric ones.
* Column lists are declared once (`NODE_COLUMNS`, `EDGE_COLUMNS`, …) and reused. Row mappers read by position, so a second hand-written copy of a column list is a live mis-mapping hazard.
* Unknown enum strings read from disk **fail loudly** rather than defaulting. An unrecognized value means the file was written by something that knew more than we do.
* Derived data — the FTS index, `cache/` — must be droppable and rebuildable at any time. Nothing may depend on it being present.
* Timestamps are RFC 3339 text and enums are lowercase strings. The file stays readable in any SQLite browser; that is worth the bytes.

---

## 6. Errors

* One error enum per crate, with a variant that names the actual condition.
* **Never reuse an unrelated variant for a new condition.** A workspace-not-found reported as a blob-store error tells the user something false.
* Never swallow errors. `.ok()` on a query discards real failures as well as "no rows" — match the not-found case explicitly and propagate the rest.
* No `unwrap`, `expect` or `panic!` outside tests, `build.rs`, and program startup. Return a `Result`.
* Error messages say what to do next where there is something to do (`run 'athenaeum init' first`, `upgrade Athenaeum to open it`).
* Tauri commands return `Result<T, String>` and are wrapped in `run_guarded`, because unwinding across the command boundary is undefined behavior.
* Do not use an error to signal a non-error outcome. A command that finds problems reports them and exits non-zero; it does not invent an error variant to carry a count.

---

## 7. Concurrency & State

* `KnowledgeStore` owns a connection and is not `Clone`. Share one behind a `Mutex`; never open the same file twice from one process.
* A poisoned lock is reported to the user as recoverable state, not unwrapped.
* WAL is on so a reader is not blocked behind a writer. Assume **the CLI and the desktop app can touch one workspace at the same time**, and write file operations accordingly — unique temp names, atomic rename, idempotent writes.
* Derive state rather than duplicating it. One source of truth.
* Never expose mutable internal state through a public API.

---

## 8. The Command Layer (Tauri)

* Every command is a **thin translation between JSON and the library**. If a rule about how knowledge behaves lands in `commands.rs`, it belongs in `athenaeum-core`.
* Every command returns a **typed** struct. No hand-built `serde_json::Value` with hand-written keys — that is an untyped contract the compiler cannot check.
* Vocabularies are served from the library, never duplicated in TypeScript.
* `src/api.ts` mirrors the registered command surface exactly. A command with no binding, or a binding with no command, is drift.
* Capabilities in `src-tauri/capabilities/` are **least privilege**: a permission with no caller gets removed, and so does the plugin behind it.

---

## 9. The CLI

* The CLI is the acceptance harness for the library. Every operation the desktop app performs is reachable here, with no window, no frontend build and no API key.
* `main.rs` defines arguments and dispatches. Rendering lives in `render.rs`; validation in `check.rs`.
* **Global flags are honored by every command.** `--json` is documented as global; a command that ignores it is broken for the scripting use case it exists to serve.
* Text output is for humans, JSON for machines, and the two come from the same value — never from two separately-constructed representations.
* Accept either an id or a title wherever a node is named.

---

## 10. The Desktop Frontend

Deliberately minimal — the graph view is not the product, and a beautiful visualization over an empty database demonstrates nothing. "Minimal" governs *scope*, not quality.

* **`App.tsx` composes. Components display. Hooks orchestrate.** A component must not call `api.*` or hold application state; a hook owns async state; `App.tsx` assembles them.
* Components take typed props and emit callbacks. They never know the Rust command surface.
* Async work handles `idle → loading → success | error` explicitly, and deliberately handles **stale responses, duplicate requests and races**. Two searches in flight must not let the older one win.
* Never swallow an error into a silent `catch {}` — surface it in the error banner.
* Stable `key`s from real identity. Never an array index for a list that can reorder.
* Styling is one plain stylesheet with CSS custom properties for color. Use the existing variables; no raw hex inside a component, no second styling system.
* The window is resizable down to 720×480: layouts use flex/grid and intrinsic sizing, never fixed offsets. Mobile breakpoints are not applicable — this is a desktop window.
* Focus stays visible, controls have accessible labels, and important state is not signalled by color alone.
* Every control works, is disabled, or is removed. No placeholder UI.

---

## 11. Cross-Platform

The same knowledge base must open unchanged on Windows, macOS and Linux. What differs is confined to `src-tauri/tauri.*.conf.json`.

* SQLite is compiled from source and linked statically (`rusqlite`'s `bundled`), so FTS5 is present everywhere rather than depending on the host.
* TLS is rustls, never native-tls. No OpenSSL to find.
* **Anchored paths are normalized to forward slashes on the way in.** A graph written on Windows must resolve on macOS.
* No platform-conditional knowledge logic. If a `#[cfg(windows)]` appears in `athenaeum-core`, something is wrong.
* Anything touching the filesystem is tested on all three platforms in CI, not just the one you develop on.

---

## 12. Performance

Optimize **real or obvious bottlenecks**, not code for appearance.

Check:

* accidental O(n²) — a linear scan inside a loop over real data; build a `HashMap`/`HashSet`
* N+1 queries in a loop over nodes or edges
* work repeated per iteration that could be hoisted
* unbounded traversal, unbounded result sets, unbounded briefs
* loading a whole table into a `Vec` when the rows could stream
* React: expensive computation in a render body, unstable props to memoized children, unnecessary re-renders

Use when justified: `HashMap`/`HashSet`, a single batched query, an index, a budget, memoization.

Budgets are load-bearing here, not an optimization: an unbounded walk of a mature graph is "dump the database into the model", which is the failure this design exists to avoid. Do not raise a budget to make a test pass.

Do not micro-optimize cold paths or add memoization to trivial work.

---

## 13. Security

* Never hardcode secrets, credentials, tokens, or production URLs. API keys belong in the app's settings, never in the repo.
* Never commit `.env` or a knowledge base (`.athenaeum/`).
* **Retrieved knowledge is reference material, never instruction.** Anything reaching the graph may be untrusted — READMEs, code comments, git history, model output. `KnowledgeBrief::to_prompt()` states that boundary explicitly; do not remove it, and do not add a path that hands graph content to a model without it.
* Validate external input at boundaries: user text into FTS5 is sanitized to terms, not passed as query syntax.
* Parameterized SQL only.
* Least-privilege Tauri capabilities; keep the CSP restrictive.
* Never use `npm audit fix --force` without approval. Document unresolved vulnerabilities.

---

## 14. Testing & Verification

Logic changes require tests, especially for:

```text
adapters and row mappers
parsing and vocabularies
store behavior: idempotency, cascades, transactions
traversal, inverses, budgets
authority resolution
edge and error cases
```

Tests must run **offline and without an API key**. That is a hard constraint, not a preference — it is what makes the library provable on all three platforms in CI.

`crates/athenaeum-core/tests/knowledge_loop.rs` is the acceptance test for the product thesis: session one learns, session two navigates to it without rediscovering. If it fails, the product does not work, however good everything else looks.

### The gates

These are the real commands. All of them must pass.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo test --workspace --doc
npm run build             # tsc --noEmit, then the frontend bundle
```

CI additionally runs the tests on Windows, macOS and Linux, smoke-tests the built CLI end to end, and regenerates the icons to prove they still match `scripts/generate_icons.py`.

Note that `cargo fmt` does not reach inside `macro_rules!` invocations — indentation in a `str_enum!` block is on you.

Never claim success when a gate fails. Report the exact command and result.

---

## 15. Code Review

Priority:

```text
correctness
→ architecture and layer boundaries
→ data integrity and schema safety
→ error handling and concurrency
→ performance
→ cross-platform behavior
→ maintainability
→ polish
```

Severity:

```text
P0  incorrect behavior, data loss, security, swallowed errors, races,
    an error that tells the user something false, knowledge logic outside the library
P1  layering violations, a god-file mixing concerns, missing journal entries,
    unhandled stale async, O(n²) on real data, missing tests for changed logic
P2  duplication, magic values, dead code, unused dependencies and permissions,
    untyped contracts, needless fragmentation
P3  naming, documentation, formatting, minor polish
```

Every finding must identify:

```text
path:line
→ problem
→ why it matters
→ specific fix
```

Do not report vague quality concerns.

---

## 16. Refactoring

Refactors preserve behavior unless a bug fix is explicitly requested, and each bug fix is called out.

For structural moves:

```text
map old path → new path
→ resolve imports through the map, never by string substitution
→ move with git mv
→ update references, docs and CI paths
→ run the gates
```

Do not combine unrelated behavioral changes with a structural move — a move diff that also changes behavior is unreviewable.

Do not restructure vendored files (§2).

---

## 17. Finish Standard

Before reporting completion:

```text
inspected
→ changed
→ self-reviewed
→ verified
→ files identified
→ gates passed
→ limitations stated
→ no unrelated features added
```

Final check:

```text
correct crate and layer?
smallest correct change?
would this survive on all three platforms?
typed at every boundary?
single source of truth?
transactional, and journaled?
errors honest?
budgets respected?
tests offline?
no duplicate system?
no speculative features?
gates passed?
```

For meaningful work, update the phase table under *Where this sits against the
build plan* in [`docs/architecture.md`](architecture.md), so the next session
starts from what is actually true.

`docs/vision/build-plan.md` is the cross-repo vision document the code cites as `§N`. It is a
record of what was decided, not a progress log — read it, do not edit it.

### Completion Report

Keep it brief:

```text
changed
why
files
verification
limitations
```

**Never say "done" unless the required verification gates pass.**
