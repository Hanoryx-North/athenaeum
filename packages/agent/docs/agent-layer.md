> Current source layout and root commands: [the consolidation guide](../../../docs/consolidation.md).
> Older sibling-checkout and pnpm development examples below are historical.

# The agent layer

This repository is the layer between a coding agent and a project's knowledge base. It is a
[DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness) bundle: four plugins that
add one capability to a harness that already owns the agent loop, tools, permissions,
sessions and providers, plus a fifth row that configures the harness's own delegation tool
into the Wiki Agent role.

```text
┌──────────────────────────────────────────────────────────────┐
│ DeepSeek Harness                                             │
│ agent loop · tools · permissions · sessions · model adapters │
│                                                              │
│   ┌──────────────────────────────────────────────────────┐   │
│   │ athenaeum-agent (this repository)                    │   │
│   │                                                      │   │
│   │  wiki             Service Definition   ctx.wiki      │   │
│   │  wiki-athenaeum   Service Provider     protocol v1   │   │
│   │  tool-wiki        Consumer             wiki_* tools  │   │
│   │  wiki-context     Consumer             the brief     │   │
│   └───────────────────────┬──────────────────────────────┘   │
└───────────────────────────┼──────────────────────────────────┘
                            │ JSON over `athenaeum bridge`
                            ▼
              ┌──────────────────────────────┐
              │ Athenaeum knowledge library  │
              │ graph · evidence · history   │
              └──────────────────────────────┘
```

Why the runtime is not built here is recorded in [runtime-pivot.md](../../../docs/consolidation.md).

## The seam

A capability in the harness has three roles, and a seam is all three or it is not a seam.

| Role | Module | Owns |
| --- | --- | --- |
| Service Definition | `athenaeum-agent/wiki` | The `ctx.wiki` interface, the provider registry, execution-time provider selection, and the probe that says whether a knowledge base is there at all |
| Service Provider | `athenaeum-agent/wiki-athenaeum` | Wiki protocol v1 over the `athenaeum bridge` command, and capability negotiation |
| Consumer | `athenaeum-agent/tool-wiki` | The model-facing `wiki_*` tools: schemas, descriptions, prompt guidance, presentation |
| Consumer | `athenaeum-agent/wiki-context` | The knowledge brief that opens a task, and the utility report that closes it |
| Configuration | `athenaeum-agent/wiki-agent` | The Wiki Agent role: the child's route, persona, tool subset and recursion budget, mounted on the harness's own delegation seam |

Splitting them is what makes the arrangement replaceable. A second knowledge library
implements `WikiProvider` and changes nothing above it; a product that wants briefs but no
knowledge tools drops one consumer row from its configuration.

`athenaeum-agent/protocol` sits under all of it as a plain TypeScript client with no harness
dependency at all, so the wire contract can be exercised against the library's published
fixtures without a running agent.

## Four decisions worth knowing about

**Selection is resolved per call and never by registration order.** A configured provider id
that is missing or unusable fails loudly rather than falling back to another. Silently
consulting a different knowledge base than the one configured is worse than consulting none.

**The brief is a user message, not a system-prompt section.** It answers one task, so a
cached system prompt holding it would be invalidated every turn. A durable `user/message`
event is also what makes the brief reconstructable from the session log, which the harness
requires of everything a model sees.

**The injected text is the library's own trust wrapper, verbatim.** The library returns both
a structured brief and a `prompt` that states retrieved knowledge is project reference
material — that it does not override system instructions, grant permissions, or authorize
tool execution. Re-rendering the structure here would drop that framing, so this layer never
does. The permission engine never consults the brief either, so the wrapper is a second line
rather than the only one.

**Utility reports what the answer cited, not what the brief carried.** A node counts as used
when the turn's output names it by id or title. Reporting every retrieval as a success would
destroy the one signal that separates knowledge worth keeping from knowledge that keeps
surfacing and never helps.

## What the model gets

Read tools are registered by default:

| Tool | Does |
| --- | --- |
| `wiki_search` | Find candidate seed nodes by text |
| `wiki_open` | One node with its evidence, history and utility, and the file it lives in when it is a page of the vault |
| `wiki_navigate` | Walk outward from a node under a budget |
| `wiki_path` | How two nodes connect, step by step |
| `wiki_related` | One node's direct relationships, proposals opt-in |
| `wiki_evidence` | Why a node is believed |
| `wiki_history` | What a node used to say |
| `wiki_brief` | Ask for a knowledge brief on one question, when a task arrives without one |
| `wiki_status` | Whether this project has a knowledge base at all, and how fresh its index is |

`wiki_brief` returns the library's own trust-wrapped prompt as the text the model reads, so
the reference-material framing travels with the knowledge rather than being re-rendered here.
`wiki_status` is the one knowledge tool that always answers: it reports the seam's probe state
(`ready`, `absent`, `incompatible`, `unreachable`, `unavailable`) so a model that sees
`wiki_search` error can tell a missing knowledge base from a stale CLI, and it says "never
indexed" rather than "stale" when no index run has happened — the library reports
`indexStale: false` for that case, and only `indexedAt: null` distinguishes it.

**A relationship carries its reason.** When the library records one, the `note` and
`confidence` on a related node, a walk step, or a path step are rendered into
`wiki_related`, `wiki_navigate` and `wiki_path` text: why two nodes are connected is the
useful part of the edge, and an edge nobody explained says nothing about whether it still
holds. Both fields are additive — a build that predates them sends neither and the edge
renders as it always did. `wiki_path` treats "no path within the budget" as an ordinary
answer with an ordinary sentence; only a node that does not resolve at all is `not_found`.

Every read and curation failure reaches the harness as a structured tool error
(`WikiToolError`, carrying the original protocol or bridge code), never as a message to parse.

**Pages.** A node that is a page of the project's vault is shown with its file. The tool
description and system prompt tell the model to propose changes for a person to review and to
avoid writing, moving or renaming the page without explicit permission. The path is its
identity. The node still exposes relationships and evidence, while the content hash stays out
of what the model reads.

`curation: false` stops the model *proposing* knowledge through the bridge. It does not make the vault
read-only: the pages are files in the workspace, and an agent with write access can edit them whatever
this switch says. Model-facing knowledge tools are proposal-only; direct file writes are governed
by harness permissions. Until a deployment protects `wiki/` writes with a harness permission
policy or documented extension, desktop agent mode cannot guarantee person review. This bundle
does not patch the harness to impose that policy.

Curation tools — `wiki_create`, `wiki_link`, `wiki_attach_evidence`, `wiki_proposals` — default
off in the adapter. The shipped Athenaeum profile turns them on and names the curator model, so
normal desktop and CLI sessions can prepare proposals. Other deployments can turn them off or
choose another curator model. An agent that records every hunch fills the knowledge base with
noise, and noise is what makes a knowledge base stop being worth reading.

**Nothing here accepts a proposal.** Every curation call is attributed to the configured model
name and to the session, and the library stores what it receives as a proposal: a created node
starts with unverified authority, a created relationship stays proposed and is not traversed,
and the tools offer no `accept` or `reject`. Acceptance is a person's review in Athenaeum, and
post-task discoveries therefore become attributed proposals or they become nothing. The
protocol still carries `accept` and `reject` so a UI can drive that review; the model never
gets them.

## The Wiki Agent role

The primary agent can delegate one knowledge question to a child agent instead of walking the
graph itself, which is what makes retrieval cheap enough to use on every task. The role is
configuration, not a second runtime: `athenaeum-agent/wiki-agent` mounts the harness's own
delegation tool with this bundle's settings.

| Field | Default | Meaning |
| --- | --- | --- |
| `enabled` | `true` | Register the role; `false` keeps the row mounted but inert |
| `toolName` | `wiki_agent` | Model-facing name of the delegation tool |
| `provider` | `spawn` | The `ctx.subagents` provider that runs the child |
| `modelProvider` / `model` | unset | The child's LLM route; set together, or omit both to inherit the parent's route |
| `reasoningEffort` | unset | Adapter-owned effort for the child's route |
| `maxTokens` | unset | Output cap for one child request |
| `persona` | knowledge-navigator role | Persona shadowing the deployment persona for the child only |
| `maxDepth` | `0` | Recursion cap; `0` keeps the navigator from delegating further |
| `tools` | the `wiki_*` read tools | Global tools the child keeps; everything else is removed |
| `backgroundMode` | `one-shot` | Whether the delegated call waits for its result |

The extension points it uses are the harness's, and they are documented ones:

- `ctx.subagents` — `@deepseek-ai/dsh-subagent`, the delegation service and its provider
  registry. The profile's base bundle (`@deepseek-ai/dsh-base`) already mounts it together
  with the in-process `spawn` provider.
- `@deepseek-ai/dsh-tool-subagent` — the model-facing half of that seam. Its configuration
  carries the child's `agentOptions` route, `persona`, `toolFilter`, `maxDepth` and
  `backgroundMode`, and it validates a configured route against the live model adapters before
  a child starts.

The bundle's plugin does that validation itself, at load, and mounts the row as an effect:
`modelProvider` without `model` (or the reverse), an empty tool subset, and a negative
`maxDepth` all refuse the load rather than failing the first delegation. On a profile whose
base bundle mounts no delegation seam the plugin never applies, and deterministic retrieval
through `wiki_*` is unchanged — the role is an accelerator, never a prerequisite.

A profile configures it the way it configures every other row:

```yaml
- id: wiki-agent
  config:
    enabled: true
    toolName: wiki_agent
    provider: spawn          # the ctx.subagents provider: spawn starts a fresh in-process child
    modelProvider: deepseek  # the child's route; set with `model`, or omit both to inherit the parent's
    model: deepseek-v4-flash
    maxDepth: 0              # the navigator answers; it does not delegate further
    tools:                   # exactly the tools a knowledge navigator needs (this is the default set)
      - wiki_search
      - wiki_open
      - wiki_navigate
      - wiki_path
      - wiki_related
      - wiki_evidence
      - wiki_history
      - wiki_brief
      - wiki_status
```

`tools` becomes the harness row's `toolFilter.allow`: the child keeps those global tools and
loses every other, so it cannot write files, run commands, record knowledge, or delegate
again — and an allow-list naming a tool that is not registered fails the delegation loudly
rather than silently giving the child more than intended.

What the harness does **not** offer, so nobody assumes otherwise:

- **No per-role model routing outside delegation.** `ctx.agentDefaultModel` is a Host-wide
  default consulted when an Agent is created; it is not a per-role switch, and mounting it
  inside a composition does not re-route one agent (ordinary service lookup still selects the
  Host realm). A child's route is supplied per delegation, which is exactly what
  `agentOptions` above does. There is no "the wiki agent uses model X" setting that applies to
  a session the bundle did not create.
- **No session-level role for a bundle to select.** *An Agent's* tools, prompt sections and
  skills come from a preset, and a session chooses its preset through
  `@deepseek-ai/dsh-agent-preset-registry`; a bundle patch can declare such a row, but it
  cannot select one for a session. That is why this bundle ships the delegation form and not a
  preset: the product's flow is the primary agent asking a cheaper child, and the delegation
  row expresses it without a second composition that would have to be selected by hand.
- **No permission by prompt.** Nothing in the role's persona grants or withholds anything. A
  tool filter is applied by the harness to the child's scope; the persona is text a model may
  ignore, and it is written as guidance, never as enforcement.

## Knowing whether there is a knowledge base

`ctx.wiki.probe()` answers the question "can knowledge help here at all?" with a value instead
of an exception, so a consumer never has to catch to find out:

| State | Means |
| --- | --- |
| `ready` | A knowledge base answered; its status rides along |
| `absent` | This workspace has no knowledge base. Normal, and never a reason to stop |
| `incompatible` | The library speaks another protocol version, or its build predates an operation this bundle needs — a stale or wrong CLI |
| `unreachable` | The command did not run, exceeded its budget, was cancelled, or answered something that is not a v1 envelope |
| `unavailable` | The seam could not choose a provider, or the library answered an error that is neither of the above |

## Installing it

The bundle is out-of-tree: a harness profile installs it rather than the harness vendoring it.

```sh
dsh plugin --profile athenaeum add /path/to/athenaeum_agent
dsh --profile athenaeum --dump-config     # shows the athenaeum-agent layer
dsh --profile athenaeum "why does the OAuth callback validate state first?"
```

The knowledge base is whichever project the runtime is pointed at; set `workspace` on the
`wiki-athenaeum` row to address one somewhere else. Every row this bundle inserts can be
overridden from the profile's own `cordis.patch.yml` — see
[`cordis.patch.yml`](../cordis.patch.yml) for what it contributes.

## Configuration

| Row | Field | Default | Meaning |
| --- | --- | --- | --- |
| `wiki` | `provider` | auto-select | Which registered provider answers, when several are mounted |
| `wiki-athenaeum` | `id` | `athenaeum` | Seam registry id |
| `wiki-athenaeum` | `command` | `athenaeum` | The executable; a bare name resolves through `PATH` |
| `wiki-athenaeum` | `workspace` | working directory | The project whose knowledge base is addressed |
| `wiki-athenaeum` | `timeoutMs` | `30000` | Budget for one bridge call |
| `wiki-athenaeum` | `maxResponseBytes` | `4194304` | Cap on one bridge response |
| `tool-wiki` | `read` | `true` | Register the read tools |
| `tool-wiki` | `curation` | `false` | Register the curation tools |
| `tool-wiki` | `curatorModel` | — | Model name attributed to proposals; required when `curation` is on |
| `tool-wiki` | `timeoutMs` | `30000` | Budget for one knowledge tool call |
| `wiki-context` | `depth` | library default | Relationship hops the brief walks; a positive integer |
| `wiki-context` | `maxNodes` | library default | Upper bound on nodes in one brief; a positive integer |
| `wiki-context` | `recordUse` | `true` | Report which briefed nodes the turn cited |
| `wiki-agent` | `enabled` | `true` | Register the Wiki Agent role |
| `wiki-agent` | `toolName` | `wiki_agent` | Model-facing name of the delegation tool |
| `wiki-agent` | `provider` | `spawn` | The `ctx.subagents` provider that runs the child |
| `wiki-agent` | `modelProvider` | — | Child LLM provider route; set together with `model` |
| `wiki-agent` | `model` | — | Child model id; set together with `modelProvider` |
| `wiki-agent` | `reasoningEffort` | adapter default | Reasoning effort for the child's route |
| `wiki-agent` | `maxTokens` | adapter default | Output cap for one child request |
| `wiki-agent` | `persona` | knowledge-navigator role | Persona for the child only |
| `wiki-agent` | `maxDepth` | `0` | Recursion cap; `0` forbids further delegation |
| `wiki-agent` | `tools` | the `wiki_*` read tools | Global tools the child keeps |
| `wiki-agent` | `backgroundMode` | `one-shot` | Whether the delegated call waits for its result |

## When there is no knowledge base

Every path degrades to "the agent works exactly as it would without this bundle":

- **No provider mounted.** The seam has nothing to select; the tools report a structured
  error if the model calls one, and no brief is injected.
- **A project that never ran `athenaeum init`.** `probe()` reports `absent` — from the
  library's own status when it answers one, or from `workspace_not_found` — and the provider
  treats it as "there is nothing here" rather than a failure.
- **The library is unreachable.** The brief is skipped with a warning and the turn proceeds.
  Knowledge is an accelerator; the agent can always read the repository itself.
- **The installed CLI is stale or wrong.** `probe()` reports `incompatible` and names the
  code: a protocol version this adapter does not speak, or a build that does not serve an
  operation it needs. The tools fail with that code rather than misreading an older build.
- **The knowledge base had nothing to say.** No context is added at all. An empty brief would
  tell the model the project has no conventions, which is a stronger claim than "nothing is
  recorded yet".
- **The profile has no delegation seam.** The Wiki Agent role never applies. Retrieval is
  unchanged; there is simply no `wiki_agent` tool.

## Testing

This repository ships no tests. The suite that proves this bundle lives in
`athenaeum_validation`, alongside the other repositories' tests, so the executable
verification of these paths has one owner and one place to run it. What is checked here on
every change is that the bundle typechecks and builds against the pinned harness checkout:

```sh
pnpm install
pnpm run typecheck     # tsc -p tsconfig.json --noEmit
pnpm run build         # tsc -p tsconfig.build.json
```

The layers that suite covers, in order of how much they prove:

1. **Client tests** pin the wire rules — version, correlation, method agreement, payload
   presence, error classification.
2. **Fixture tests** run Athenaeum's own published protocol v1 fixtures. The request cases
   compare envelopes this repository builds against the library's documented ones, so a field
   name drifting apart from the library fails there.
3. **Seam and provider tests** cover selection, negotiation caching, the probe states and the
   degraded paths above.
4. **Consumer tests** mount each consumer as a plugin. The tools go into a real harness tool
   registry: what registers under which configuration, what the model reads back, and what a
   curation proposal carries. The brief goes onto a real `agent/pre-step` chain: which step it
   attaches to, that the library's trust wrapper is what reaches the model, and which turn its
   utility report is paired with.
5. **End-to-end tests** build a real knowledge base and a real vault with the real CLI and drive the
   real bridge, including the page pointer and an edit or a new page seen with no re-index.
6. **The harness test** boots the real `dsh` runtime with this checkout installed as a bundle, the
   real library behind it and the harness's own scripted model in front. It is the only layer that
   notices when the harness changes an API this bundle uses.

The end-to-end suite includes the product thesis as an assertion: a decision whose wording
shares no term with the task is absent from search results and present in the brief, because
the relationship — not the similarity — is what reaches it.

## Development

Development resolves the harness from a sibling checkout of the release pinned in `harness.pin.json`.
[development.md](development.md) has the layout, the build order and why each step is there,
and the failures worth knowing about. The test suite that runs against this bundle, `pnpm verify`
included, lives in `athenaeum_validation`.
