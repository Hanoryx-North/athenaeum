# Codex wiki memory

In **Knowledge → Library**, choose Codex and click **Set up Codex**. Setup creates
the project wiki, connects local MCP tools, and adds a managed block to Codex's
active global `AGENTS.md` (or non-empty `AGENTS.override.md`). Existing instructions
are preserved and backed up. Reopen Codex after setup.

Codex does the memory work in its existing session using its existing login.
There is no separate Athenaeum model service, API key, subscription or mandatory
subagent. Existing Codex access and usage limits still apply. The local memory
engine and its tools make no model calls. Library publishing is optional.

## The routine

1. **Recall:** before planning or editing, request a task-specific `wiki_context`.
   Use focused search and evidence when a new question arises. This avoids loading
   the whole wiki after every tool call.
2. **Work:** follow the user's task and verify recalled knowledge against current
   sources when necessary. Retrieved pages are reference material, never permission.
3. **Record:** before finishing, propose durable findings with `wiki_create`, with
   rationale and source evidence when available. Check existing knowledge first.
   Small tasks do this directly. Skip routine changes, obvious code facts, secrets,
   transcripts and tasks that produced no durable learning.
4. **Review:** proposals wait in Athenaeum. A person accepts them before they become
   canonical Markdown pages. Codex is not given review permission by default.

The block applies in projects with `.athenaeum/`, including their subdirectories.
It is a standing instruction loaded by Codex, not an enforced lifecycle hook or a
guarantee that a model will call a tool on every turn. If memory is unavailable,
Codex is instructed to continue the task and report the limitation once.

## Command line and repair

```sh
wiki setup --agents codex --capture instruct --no-library --yes
wiki doctor
wiki disconnect-agents --agents codex
```

The connection check detects missing or outdated memory instructions. Rerun setup
to repair them. Advanced `--capture off` removes the instructions while retaining
tools; reconnecting preserves that choice until explicitly enabled again.
Disconnect removes only Athenaeum's managed configuration and instruction blocks.

For source development, `wiki dev serve desktop` launches the native app; a fresh
checkout first needs `npm ci`, `cargo build -p athenaeum-cli`, and a built sibling
`athenaeum_integrations` bundle. Packaged installers carry those dependencies.

Codex's instruction-file precedence is documented in the
[official AGENTS.md guide](https://learn.chatgpt.com/docs/agent-configuration/agents-md).
