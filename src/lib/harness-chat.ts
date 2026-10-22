/**
 * Chat-side bookkeeping for the DeepSeek Harness runtime the desktop driver owns.
 *
 * A harness session id can be created once: the runtime persists each session and
 * refuses the same id again after it restarts. The driver keeps one runtime and
 * replaces it on Stop (the SDK's only cancel), on a project switch, and when it
 * crashes, so a conversation gets a fresh session per runtime and carries its
 * earlier messages into it. Nothing here decides anything about a turn.
 */

export interface HarnessHistoryMessage {
  role: "user" | "assistant"
  content: string
}

const sessions = new Map<string, string>()
let sessionsProject: string | null = null

/**
 * The conversation's session on the current runtime, and whether it is new.
 * Asking for another project means the driver replaced the runtime.
 */
export function harnessSessionFor(projectPath: string, conversationId: string): { sessionId: string; fresh: boolean } {
  if (sessionsProject !== projectPath) {
    sessions.clear()
    sessionsProject = projectPath
  }
  const existing = sessions.get(conversationId)
  if (existing) return { sessionId: existing, fresh: false }
  const sessionId = `${conversationId}-${crypto.randomUUID().slice(0, 8)}`
  sessions.set(conversationId, sessionId)
  return { sessionId, fresh: true }
}

/** Forget every session: the runtime that held them was, or may have been, replaced. */
export function forgetHarnessSessions(): void {
  sessions.clear()
}

/** Earlier turns, carried into a fresh session so the model keeps the conversation. */
export function historyPreamble(history: HarnessHistoryMessage[]): string {
  const turns = history
    .filter((message) => message.content.trim().length > 0)
    .map((message) => `${message.role === "user" ? "User" : "Assistant"}: ${message.content.trim()}`)
  if (turns.length === 0) return ""
  return [
    "Earlier in this conversation (restored after the agent restarted):",
    "",
    ...turns,
    "",
    "---",
    "",
    "",
  ].join("\n")
}

const SUMMARY_MAX_CHARS = 120
// The argument that best says what a call does, in order of preference.
const SUMMARY_ARGUMENTS = ["description", "command", "query", "pattern", "file_path", "path", "url", "node", "name", "prompt"]

function clip(text: string): string {
  const line = text.replace(/\s+/g, " ").trim()
  return line.length <= SUMMARY_MAX_CHARS ? line : `${line.slice(0, SUMMARY_MAX_CHARS - 1)}…`
}

/** A one-line label for a tool call: its name and the argument that says what it does. */
export function describeToolCall(tool: string, input: string): string {
  let args: unknown
  try {
    args = JSON.parse(input)
  } catch {
    return input.trim() ? `${tool} · ${clip(input)}` : tool
  }
  if (typeof args !== "object" || args === null || Array.isArray(args)) return tool
  const record = args as Record<string, unknown>
  const key = SUMMARY_ARGUMENTS.find((name) => typeof record[name] === "string" && (record[name] as string).trim())
  return key ? `${tool} · ${clip(record[key] as string)}` : tool
}

/** A one-line label for a tool result: its name, outcome and the start of its output. */
export function describeToolResult(tool: string, output: string, failed: boolean): string {
  const first = output.split("\n").find((line) => line.trim()) ?? ""
  const label = failed ? `${tool} failed` : tool
  return first ? `${label} · ${clip(first)}` : label
}
