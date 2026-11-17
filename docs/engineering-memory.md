# Engineering memory: audit and first implementation

This is a foundation milestone, not a claim that autonomous engineering improvement
or cross-project reasoning is finished. No external model account is required by
the tests; model responses in the regression suite are controlled fixtures.

## What the implementation already did well

- `athenaeum-core` owns the graph, authority, evidence, retrieval journal and
  rebuildable SQLite index. Accepted Markdown retains identity and evidence across
  rebuilds. Keep this engine and its versioned bridge.
- `portable/memory.rs` separates proposal, acceptance and verification. Acceptance
  does not make model advice authoritative. The worker has proposal permissions,
  and the owner's existing automation policy controls page saving. Keep this.
- `brief.rs` already uses summaries, bounded navigation and explicit trust framing.
  Full pages are available through follow-up tools. There is no need for a model
  on every search or a second vector database.
- `src/lib/ingest.ts` already segments long source files, checkpoints analysis,
  retains technical examples and generates schema-routed wiki pages. Its file
  ingestion workflow remains useful; replacing it with another document service
  would lose mature functionality.
- The owned runtime already has a separate, configurable background model, a
  persistent job queue, cancellation, an exclusive worker lock, and an isolated
  tool-free Codex extraction workspace. Account/model validation already occurs
  before enabling the desktop worker. It never needs to borrow the coding model.

## Root causes found

| Path | Problem before this milestone | Consequence |
| --- | --- | --- |
| `runtime/src/memory-service.mjs` | Only successful commands were collected; large early output could consume the turn budget | Debugging failures and later outcomes disappeared |
| `runtime/src/learning.mjs` | First eight files, isolated non-comment lines, at most two candidates | Architectural reasons, failure sequences and topic coverage were restricted before the model could assess them |
| Same | Prompt asked for generic summary/content; source registry and excerpts were appended to every body | Vague lesson text and repeated provenance dominated the knowledge view |
| Same | Only current-file observations could support a candidate | Test failures and tooling friction could not become properly qualified evidence |
| `runtime/src/mcp.mjs` | Automatic recall on every text turn, up to twelve nodes/two hops/32 KiB | Empty repeated checks and excessive context were permitted |
| `integrations/src/connect/instructions.ts` | Recall requested even when a host hook/runtime had already recalled | Duplicate lookups were encouraged |
| `integrations/src/hooks/index.ts` | Large briefs were sliced and a closing wrapper added | Warnings and relationships could be silently omitted |
| `core/src/store/search.rs` | Body/source-path overlap alone established recall relevance | Registry text could seed useless context |
| Node scope model | Scope is an enum; it lacks explicit project/organization identities and technology/task selectors | Shared-wiki cross-project filtering cannot be guaranteed by the current scope field |
| Native runtime vs optional Harness | Harness has a Wiki Agent delegation role; the native path uses its own worker | Configuring the optional role does not configure native background capture |

Source-file ingestion is a different workflow from reviewed agent proposals. The
former generates wiki files; the latter proposes knowledge through the engine.
The new explicit text intake uses the latter for users who want to inspect lessons
before saving them. Both use the same engine, graph and vault. The intake chunker
lives in the runtime because that process cannot depend on frontend/Tauri modules;
it prepares evidence, not another searchable document store.

## Implemented behavior

### Structured, supported lessons

The worker asks for a recommendation, rationale, applicability, validation and
limits, with optional technical example and a reviewable improvement suggestion.
Missing/shallow structural fields, unknown evidence references, invalid shapes,
common credential patterns and oversized outputs are rejected deterministically.
This is a structure/evidence gate, not proof that the interpretation is true.

Session capture uses multi-line observed blocks, including comments that explain
decisions, distributed across files under the existing 24 KiB source budget.
It records coverage counts when that budget excludes observations. Checked source
excerpts are re-read before proposing. Command observations preserve actual exit
status, bounded beginning/end output from Codex collection, and truncation state.
They are recorded as external session evidence, not current-file verification.

The model sees bounded task/outcome data, scrubbed source blocks, a bounded set of
commands and related knowledge, not the raw transcript. Common credential patterns
and private paths are screened before provider input. This screening cannot prove
arbitrary source text contains no confidential information; background processing
remains explicitly enabled by the user.

Source excerpts and workspace details now live in evidence rather than being
copied into the lesson body. Existing node types, scopes, review actions, Markdown
serialization and graph link proposals are reused. No schema migration is needed.

### Explicit text intake

In **Review → Extract knowledge from text**, supply a source title, up to 64 KiB
of text, and project or general applicability. This uses the separately configured
memory model in **Coding agents → Background memory**. It refuses to submit into
a different configured wiki. Explicit text submission works with session capture disabled. It does not switch model accounts or enable capture.

Text is split at paragraph/newline boundaries where possible. Every character,
including oversized paragraphs, Unicode and the final section, is analyzed.
Previous context is supplied only to explain split concepts. Each chunk is asked
for zero to four supported lessons; four is a ceiling, not a target. Evidence ids
are restricted to that chunk. A 64 KiB input uses at most eleven model calls with
the current boundary policy; each has a 4096-token output limit and 180-second
timeout. The queue holds at most eight pending jobs and processes serially.

Completed chunk counts are checkpointed. Exact source submissions are deduplicated
by hash, and duplicate titles within an intake are suppressed. Existing search and
review checks handle related/conflicting knowledge; this is not semantic duplicate
proof or a new conflict-resolution algorithm. Relationships must name supplied
existing nodes. Cross-chunk links between newly proposed concepts are future work.

Raw input is private local pending-job data and is deleted on completion or failure;
evidence excerpts and a source hash remain for review. A canceled job remains
pending for restart. A crash during the creation of a proposal can leave partial
side effects: creation/evidence attachment are separate bridge operations. Do not
advertise exactly-once proposal creation or blindly replay failed jobs. Checkpoints
avoid repeating fully completed chunks, not every possible interrupted mutation.

The existing automatic saving setting still applies. Turn it off for manual review
before accepting the lessons. A suggestion for a lint/test/API/CLI/skill/workflow
improvement remains unverified advice and does not install enforcement.

CLI equivalent (the same cataloged native operation used by the UI):

```sh
wiki desktop actions background_memory --json
wiki desktop invoke background_memory --stdin --json < intake.json
```

`intake.json` contains the named arguments:

```json
{
  "projectPath": "/absolute/project",
  "action": "ingest",
  "document": {
    "title": "Engineering talk",
    "text": "The source text to analyze...",
    "scope": "project",
    "wikiRoot": "/absolute/project"
  }
}
```

### Selective, quiet recall

Native automatic recall now requests five nodes and one graph hop and accepts at
most 8 KiB of complete framed context. Short acknowledgements skip retrieval.
Empty exact-task results are cached for 60 seconds, connection failures for 15;
the cache stores a task/configuration hash and diagnostics, never knowledge bodies.
Repository/configuration changes, a new task, expiry, or a successful wiki mutation
trigger fresh lookup. Manual tools remain available. A newly added external memory
may be invisible to an identical task for up to the cache TTL.

Claude hooks use the same node/hop budget, cache exact empty tasks per workspace
and session, and omit oversized briefs rather than cutting their warnings off.
Managed instructions recognize automatic recall, reducing redundant model-driven
lookups. Ordinary absence is quiet; failures remain available in diagnostics and
errors blocking an explicit memory request are visible. These policies do not
eliminate MCP process startup costs or give Athenaeum control over Codex's own
prompt construction. The optional Harness path has separate defaults.

The Rust engine requires at least one meaningful title/summary task match before
body overlap qualifies an automatic seed. Interactive search can still find source
paths. This raises precision at the cost of missing body-only lessons with poor
summaries. It does not implement semantic retrieval or applicability filtering.

## Verification and interpretation

Regression tests cover structured lessons, stale/unobserved evidence, private
paths, credential patterns, command failures, legitimate repeated commands,
bounded friction leads, exact text coverage, Unicode/tails, intake opt-in and wiki
selection, chunk checkpoints, duplicates, model routing, unavailable tools, quiet
empty recall, cache expiry/isolation, and complete trust framing. Existing engine,
protocol, proposal-to-Markdown and agent integration suites remain the authority
for persistence and compatibility.

The repeat-empty scenario reduces six context requests to one within the TTL.
The native automatic node ceiling drops from twelve to five and accepted context
from 32 KiB to 8 KiB. These are measured deterministic/budget improvements, not
evidence of better real coding-task outcomes. Controlled provider fixtures verify
orchestration and validation; they do not measure the extraction accuracy of an
actual Luna account. A live quality benchmark needs an explicitly configured
provider, a representative private corpus and reviewed expected lessons.

## Next milestones

1. Add optional, Markdown-round-trippable applicability: project identity,
   technology selectors and task circumstances, with matching in the Rust engine
   **before** seed limits and graph expansion. Scope specificity breaks ties only
   among applicable claims. Unknown organization/project identity must not broaden
   authority. Migrate historical workspace evidence conservatively. This is the
   prerequisite for reliable cross-project initialization guidance.
2. Add bridge-level idempotent proposal bundles containing evidence/relationships.
   This removes the partial-mutation window and supports safe chunk-level retries.
   Add explicit refine/merge/conflict workflows using existing review infrastructure.
3. Evaluate real extraction quality on diverse completed sessions and technical
   documents: useful-lesson coverage, unsupported-claim rate, duplicate rate,
   applicability errors, retrieval precision, latency, model usage and human edits.
   Compare coding-task success with/without memory; title citations alone are not
   proof of usefulness.
4. Aggregate reviewed friction evidence across sessions using existing evidence
   and journal records. Require a reproducible failure and measured outcome before
   recommending enforcement. Test/lint/API changes still follow normal approval
   and validation; a repeated command pattern alone never becomes a rule.

Do not add autonomous enforcement, unconditional model-based reranking, more node
types for each task stage, or automatic generation of project architecture templates
before these trust and evaluation foundations are in place.
