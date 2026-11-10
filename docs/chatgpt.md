# ChatGPT in Athenaeum

Open Connectors, choose Coding agents, add ChatGPT and continue. Athenaeum
detects an existing local ChatGPT sign-in or opens the browser sign-in flow.
Connect the project wiki, then choose Open chat. ChatGPT also appears in the
chat's Agent selector. No OpenAI API key is required for this route.

The official Codex runtime must be installed. Athenaeum discovers its executable
on PATH or its npm installation. `ATHENAEUM_CODEX_BIN` can name an executable or
`bin/codex.js`; shell wrappers are not accepted. Athenaeum keeps its owned agent
as the default, and uses Codex only when ChatGPT is selected.

OpenAI's runtime owns credentials, refresh, sessions, coding tools and model
requests. Athenaeum adapts streamed messages and tool approvals into its chat.
ChatGPT sign-in is shared with local Codex clients. Signing out here also signs
those clients out. Existing ChatGPT conversations are not imported, and wiki
building continues to use the separate AI model settings.

Each ChatGPT thread receives an MCP override for the selected project's wiki.
The override is passed to the runtime without changing global Codex settings.
Wiki tools use the versioned bridge with proposal grants. Proposed wiki pages
remain reviewable. Native agent state stays in `ATHENAEUM_AGENT_HOME`.

## CLI operations

Discover signatures with `wiki desktop actions chatgpt --json`. Read account
metadata without exposing credentials:

```powershell
'{"projectPath":"C:/projects/example","action":"status"}' | wiki desktop invoke chatgpt_account --stdin --json
```

The native account operation accepts `status`, `start`, `cancel` and `logout`.
`start` returns a browser URL and login ID. `cancel` requires that login ID.
`chatgpt.signIn` opens the sign-in URL, and `chatgpt.check` reads account status.
`agent-runtime.setRuntime` selects `native`, `chatgpt` or `harness`.
Streaming chat, cancellation and approvals use the existing agent commands.

Verification covers the driver protocol, authentication metadata filtering,
streaming, project wiki overrides, denied approvals and cancellation in
`tests/chatgpt-driver.test.ts`. The setup smoke test covers browser sign-in,
disabled connection before authentication and opening ChatGPT chat.
