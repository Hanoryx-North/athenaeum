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
