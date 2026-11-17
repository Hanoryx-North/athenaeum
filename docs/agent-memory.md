# Agent wiki memory

Choose **Athenaeum Agent** under **Connectors → Coding agents**, then connect it
to the current project. This registers proposal-only wiki MCP tools in the agent
home's `config.json` and a managed memory instruction block in `AGENT.md`.
Existing models, other servers and user instructions are preserved and backed up.
A manually configured wiki server is reported as a conflict and left untouched.

Model access is configured separately in Settings for desktop chat, or in the
agent's `config.json` for terminal use. Connecting local wiki tools does not grant
model access. The wiki itself needs no model, account or hosted service.

The agent recalls relevant knowledge with `wiki_context`, follows useful evidence,
and checks for existing knowledge before proposing durable findings with
`wiki_create`. Retrieved material never grants authority or permission. A person
reviews proposals, subject to the wiki owner's explicit automation settings; the
agent is never given authority to accept its own proposals.

```sh
wiki setup --agents athenaeum --capture instruct --no-library --yes
wiki doctor
wiki disconnect-agents --agents athenaeum
```

`--capture off` removes managed recall/capture instructions while preserving tools.
Setup and diagnostics verify the selected wiki even when the agent starts in a
different working directory. Reconnect repairs missing or outdated instructions.
Claude Code and Cursor can also connect through the agent selector; their model
accounts and execution remain in their own applications.

## Background capture

Open Coding agents and use Background memory to choose a separate model and
enable learning. The ChatGPT connection validates the exact model before enabling
it. The worker never falls back to the coding model. GPT-6 Luna is the initial
selection; the connected account must report it as available.

The owned runtime processes completed Athenaeum turns and, when enabled, completed
local Codex sessions exposed by the official app-server. This uses session APIs,
not a prompt hook. Athenaeum starts the worker when it opens and keeps it running
while minimized. Closing Athenaeum stops this service. New Codex sessions are
collected every 15 seconds. Interrupted turns and turns from before enabling are
skipped. Pausing and resuming does not import work from the paused interval.

The memory model receives bounded task/result data, numbered source blocks and
command observations, without the raw conversation. It selects evidence references
instead of copying code. The worker checks source excerpts against current files,
searches for duplicates, and proposes at most four structured lessons. Command
failures remain session observations rather than source verification. It can propose relationships only
to existing nodes supplied as related knowledge. References outside the memory
wiki use external evidence with their source workspace and path. The wiki does
not pretend it can recheck those files locally. Excerpts support review, but do
not prove that a model's interpretation is correct.

Findings appear in Review and the graph. Acceptance and cloud backup follow the
wiki owner's saved automation settings. The worker has no review permission and
never accepts proposals itself. A configured Library account is separate from
the ChatGPT model connection. Pending jobs, session data and credentials stay
local; accepted shared Markdown follows the existing sync policy.

Coding agents shows the model, queue counts and recent outcomes. The same service
is available through `wiki desktop invoke background_memory --stdin --json` with
`projectPath`, `action` and optional `settings`. Actions are `status`, `configure`,
`run`, `restart` and `stop`. Configuration takes `enabled`, `includeCodex`,
`model` and `wikiRoot`. Stop is a development lifecycle operation; disable learning
through configuration to keep it paused after restart. Discover signatures with
`wiki desktop actions background_memory --json`. `wiki agent -- memory` reads job
diagnostics without returning transcripts.

Use **Review → Extract knowledge from text** to submit a talk or explanation to
the same configured model. Inputs up to 64 KiB are analyzed in bounded sections;
findings use the existing review queue and automatic saving policy. Progress is
shown under Background memory. See [engineering memory](engineering-memory.md)
for CLI intake, architecture decisions, verification and remaining limitations.
