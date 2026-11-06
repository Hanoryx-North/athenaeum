# Product scope and boundaries

Athenaeum is one local wiki application with an agentic coding feature. Its durable
value is Markdown knowledge, evidence, review and recall. The integrated assistant
uses the wiki through the same versioned bridge as external clients.

- `athenaeum-core` owns knowledge semantics and the rebuildable SQLite index.
  It has no network, UI or agent provider dependency.
- The desktop and `wiki` CLI expose the application. Every desktop action has a
  discoverable CLI/API equivalent.
- `packages/agent/runtime` owns model turns, local tools, approvals, cancellation
  and private conversation state. The desktop renders those events.
- `packages/integrations` owns MCP and external-client setup; it never opens the
  index directly. The optional Harness adapter uses the same bridge.
- Hosted Library/API/admin services remain separate deployments. Sync exports
  accepted shared Markdown and identity/revision metadata, never raw sources,
  indexes, credentials or agent state.

Retrieved knowledge is reference material, never instructions or permission.
A model proposes findings; the owner's review/automation policy decides saving.
Agents never approve their own memory proposals. The wiki stays usable without
a model, an agent process, an account or a network connection.

The agent is focused original application code. It does not import another coding
application's source tree, account login, terminal framework or cloud services.
Commands require approval and run with user privileges; there is no claimed OS
sandbox. See the [runtime contract](../packages/agent/runtime/README.md) for supported
APIs, tools and explicit limits.

Keep one knowledge engine and one bridge. Do not put a competing tool/provider
loop into the engine, desktop or MCP server. Add new knowledge operations to the
engine and its versioned contracts; add coding capabilities to the owned runtime.
The separate `athenaeum_core` checkout is excluded from this work.
