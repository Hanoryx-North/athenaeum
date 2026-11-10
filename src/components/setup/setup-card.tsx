import { useEffect, useState } from "react"
import { ArrowLeft, ArrowRight, CheckCircle2, Loader2, PlugZap, X } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useSetupStore } from "@/stores/setup-store"
import { useWikiStore } from "@/stores/wiki-store"
import { openConnectors } from "@/stores/connectors-store"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"
import { useUiPreference } from "@/hooks/useUiPreference"
import { useChatgptStore } from "@/stores/chatgpt-store"
import { useAgentRuntimeStore } from "@/stores/agent-runtime-store"
import { ChatgptConnection } from "./chatgpt-connection"

export const AGENTS = [
  { id: "athenaeum", name: "Athenaeum Agent", description: "Use the coding assistant built into Athenaeum." },
  { id: "chatgpt", name: "ChatGPT", description: "Sign in with ChatGPT and use it inside Athenaeum." },
  { id: "claude", name: "Claude Code", description: "Connect your existing Claude Code." },
  { id: "cursor", name: "Cursor", description: "Make your wiki available in Cursor." },
] as const
export type Agent = (typeof AGENTS)[number]["id"]

export function SetupBanner() {
  const project = useWikiStore((s) => s.project)
  const doctor = useSetupStore((s) => s.doctor)
  const cli = useSetupStore((s) => s.cli)
  const check = useSetupStore((s) => s.check)
  const [dismissed, setDismissed] = useState<string | null>(null)
  useEffect(() => {
    if (project) void check(project.path)
  }, [project, check])
  const hasAgent = doctor?.checks.some((c) => c.id.startsWith("agent-") && c.ok)
  if (
    !project ||
    dismissed === project.path ||
    (cli?.found && hasAgent) ||
    (!doctor && cli?.found !== false)
  )
    return null
  return (
    <div role="status" className="flex items-center gap-3 border-b bg-primary/5 px-4 py-2 text-sm">
      <PlugZap className="h-4 w-4 shrink-0" />
      <span className="flex-1">Connect your coding agent to get started.</span>
      <Button size="sm" onClick={() => openConnectors()}>
        Finish setup
      </Button>
      <button aria-label="Dismiss setup reminder" onClick={() => setDismissed(project.path)}>
        <X className="h-4 w-4" />
      </button>
    </div>
  )
}

/** One decision per step. Publishing and account access live on separate pages. */
export function SetupCard({ initialAgent = "athenaeum", onAgentChange, onComplete, onCancel }: {
  initialAgent?: Agent
  onAgentChange?: (agent: Agent) => void
  onComplete?: () => void
  onCancel?: () => void
}) {
  const project = useWikiStore((s) => s.project)
  const workspace = project ? normalizePath(project.path) : null
  const { cli, doctor, busy, progress, error, lastSetup, check, install, setup, cancel } =
    useSetupStore()
  const [agent, setAgent] = useState<Agent>(initialAgent)
  const [page, setPage] = useUiPreference<0 | 1 | 2>(`setup-step:${workspace}`, 0, (value) => value === 0 || value === 1 || value === 2)
  const [connecting, setConnecting] = useState(false)
  const [status, setStatus] = useState("")
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const chatgptAccount = useChatgptStore((s) => s.workspace === workspace ? s.account : null)
  const selected = AGENTS.find((item) => item.id === agent)!
  const memoryAgent = agent === "chatgpt" ? "athenaeum" : agent
  const connected = doctor?.checks.some((item) => item.id === `agent-${memoryAgent}` && item.ok) === true && (agent !== "chatgpt" || chatgptAccount !== null)
  const working = connecting || busy !== null

  useEffect(() => {
    if (workspace) void check(workspace)
  }, [workspace, check])
  useEffect(() => {
    if (workspace && agent === "chatgpt") void useChatgptStore.getState().check(workspace)
  }, [workspace, agent])
  useEffect(() => {
    setConnectionError(null)
  }, [workspace])

  async function connect() {
    if (!workspace || working) return
    if (agent === "chatgpt" && !chatgptAccount) return
    setConnecting(true)
    setConnectionError(null)
    setStatus("Preparing your connection…")
    try {
      if (
        (!cli?.found || doctor?.checks.some((c) => c.id === "integrations" && !c.ok)) &&
        !(await install(null))
      )
        return
      if (normalizePath(useWikiStore.getState().project?.path ?? "") !== workspace) return
      setStatus(`Connecting ${selected.name}…`)
      const ok = await setup(workspace, { job: "setup", agents: [memoryAgent], capture: "instruct" })
      if (normalizePath(useWikiStore.getState().project?.path ?? "") !== workspace) return
      const report = useSetupStore.getState().doctor
      if (!ok || !report?.checks.some((c) => c.id === `agent-${memoryAgent}` && c.ok)) {
        setConnectionError(
          `Could not finish connecting ${selected.name}. Check the details below, then try again.`,
        )
        return
      }
      try {
        localStorage.setItem(`athenaeum.setup.complete:${workspace}`, "1")
        localStorage.setItem(`athenaeum.setup.route:${workspace}`, agent === "chatgpt" ? "internal" : "external")
      } catch {
        /* The connection itself is already saved by setup. */
      }
      if (agent === "chatgpt") useAgentRuntimeStore.getState().setRuntime("chatgpt")
      setPage(2)
    } finally {
      setConnecting(false)
      setStatus("")
    }
  }

  if (!workspace) return null
  return (
    <section className="space-y-7" aria-label="Connect your coding agent">
      {onCancel && <Button variant="ghost" disabled={working} onClick={onCancel}><ArrowLeft className="h-4 w-4" />Coding agents</Button>}
      <ol aria-label="Connection steps" className="flex items-center gap-2 text-xs sm:gap-5">
        {["Choose agent", "Connect", "Ready"].map((label, index) => (
          <li
            key={label}
            aria-current={page === index ? "step" : undefined}
            className={cn(
              "flex items-center gap-2",
              page === index ? "font-medium text-foreground" : "text-muted-foreground",
            )}
          >
            <span
              className={cn(
                "flex h-6 w-6 items-center justify-center rounded-full border",
                index <= page && "border-primary bg-primary text-primary-foreground",
              )}
            >
              {index + 1}
            </span>
            {label}
          </li>
        ))}
      </ol>
      {page === 0 && (
        <>
          <header>
            <h2 className="text-2xl font-semibold">Which coding agent do you use?</h2>
            <p className="mt-2 text-sm text-muted-foreground">
              Choose an assistant to use in Athenaeum or connect to this project's wiki.
            </p>
          </header>
          <div className="space-y-2" role="radiogroup" aria-label="Coding agent">
            {AGENTS.map((item) => (
              <button
                key={item.id}
                role="radio"
                aria-checked={agent === item.id}
                onClick={() => {
                  setAgent(item.id)
                  onAgentChange?.(item.id)
                  setConnectionError(null)
                }}
                className={cn(
                  "flex w-full items-center justify-between rounded-xl border p-4 text-left focus-visible:ring-2 focus-visible:ring-ring",
                  agent === item.id ? "border-primary bg-primary/5" : "hover:bg-muted/30",
                )}
              >
                <span>
                  <span className="block font-medium">{item.name}</span>
                  <span className="mt-1 block text-sm text-muted-foreground">
                    {item.description}
                  </span>
                </span>
                <span
                  aria-hidden
                  className={cn(
                    "ml-4 h-4 w-4 shrink-0 rounded-full border",
                    agent === item.id && "border-[5px] border-primary",
                  )}
                />
              </button>
            ))}
          </div>
          <footer className="sticky bottom-0 z-10 bg-background pb-4 flex justify-end border-t pt-5">
            <Button onClick={() => setPage(1)}>
              Next
              <ArrowRight className="h-4 w-4" />
            </Button>
          </footer>
        </>
      )}
      {page === 1 && (
        <>
          <header>
            <h2 className="text-2xl font-semibold">Connect {selected.name}</h2>
            <p className="mt-2 text-sm text-muted-foreground">
              {connected
                ? `${selected.name} is already connected. Continue to finish, or reconnect if you need to repair it.`
                : `One click connects ${selected.name} to ${project?.name}.`}
            </p>
          </header>
          {agent === "chatgpt" ? <>
            <ChatgptConnection workspace={workspace} />
            <p className="text-sm text-muted-foreground">ChatGPT can work with this project and recall wiki knowledge. You review proposed wiki pages. Your existing ChatGPT conversations aren't imported.</p>
          </> : <div className="space-y-3 rounded-xl border bg-muted/20 p-5 text-sm">
            <p>{agent === "athenaeum" ? "Uses the model provider you choose in Settings." : `Uses your existing ${selected.name} account.`}</p>
            <p className="text-muted-foreground">
              {agent === "cursor"
                ? "Ask Cursor to search your wiki and propose useful findings."
                : "Your agent will be asked to recall relevant knowledge and propose useful findings as you work."}
            </p>
            <p className="text-muted-foreground">
              You review proposals before they become wiki pages.
            </p>
            <p className="text-muted-foreground">{agent === "athenaeum" ? "This connects wiki memory. Configure a model separately in Settings." : `This connects wiki memory. You stay signed in through ${selected.name}.`}</p>
          </div>}
          {connecting && (
            <p role="status" className="flex items-center gap-2 text-sm">
              <Loader2 className="h-4 w-4 animate-spin" />
              {status}
            </p>
          )}
          {(connectionError || error) && (
            <p role="alert" className="text-sm text-destructive">
              {connectionError ?? error}
            </p>
          )}
          <footer className="sticky bottom-0 z-10 bg-background pb-4 flex flex-wrap items-center justify-between gap-3 border-t pt-5">
            <Button variant="ghost" disabled={working} onClick={() => setPage(0)}>
              <ArrowLeft className="h-4 w-4" />
              Back
            </Button>
            <div className="flex gap-2">
              {connecting && (
                <Button variant="outline" onClick={() => void cancel()}>
                  Cancel
                </Button>
              )}
              {connected && !connectionError && (
                <Button variant="outline" disabled={working} onClick={() => void connect()}>
                  Reconnect
                </Button>
              )}
              <Button
                disabled={working || (agent === "chatgpt" && !chatgptAccount)}
                onClick={() => (connected && !connectionError ? setPage(2) : void connect())}
              >
                {working
                  ? "Please wait…"
                  : connected && !connectionError
                    ? "Next"
                    : `Connect ${selected.name}`}
                <ArrowRight className="h-4 w-4" />
              </Button>
            </div>
          </footer>
          {(connectionError || error || progress.length > 0) && (
            <details className="text-xs text-muted-foreground">
              <summary className="cursor-pointer">Connection details</summary>
              <pre className="mt-3 max-h-48 overflow-auto whitespace-pre-wrap rounded bg-muted p-3">
                {[
                  ...(lastSetup?.steps
                    .filter((step) => step.status === "failed")
                    .map((step) => step.detail) ?? []),
                  ...progress.slice(-15),
                  ...(doctor?.checks
                    .filter((c) => !c.ok && (c.id === `agent-${memoryAgent}` || c.id === "integrations"))
                    .map((c) => c.detail) ?? []),
                ].join("\n")}
              </pre>
            </details>
          )}
        </>
      )}
      {page === 2 && (
        <>
          {connected && <CheckCircle2 className="h-10 w-10 text-emerald-600" />}
          <header>
            <h2 className="text-2xl font-semibold">{connected ? `${selected.name} is connected.` : working ? "Checking connection…" : "Connection needs attention"}</h2>
            <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
              {connected ? <>
              {agent === "athenaeum" || agent === "chatgpt" ? "Open chat for" : `Reopen ${selected.name} in`} <strong className="text-foreground">{project?.name}</strong>{" "}
              {agent === "chatgpt" ? "to start a task with your ChatGPT account." : agent === "athenaeum" ? "and choose a model in Settings to start a task." : "to load the connection. Then code as usual."}
              </> : "Go back to reconnect your agent, or return to Coding agents to check its status."}
            </p>
          </header>
          <div className="space-y-3 rounded-xl border p-5 text-sm">
            <p>Try asking: “What does this project’s wiki know?”</p>
            <p className="text-muted-foreground">New findings appear in Review → Proposals.</p>
          </div>
          <footer className="sticky bottom-0 z-10 bg-background pb-4 flex justify-between border-t pt-5">
            <Button variant="ghost" onClick={() => setPage(1)}>
              Back
            </Button>
            <Button disabled={!connected || working} onClick={() => {
              setPage(0)
              if (onComplete) onComplete(); else openConnectors("agents")
              if (agent === "chatgpt") {
                useAgentRuntimeStore.getState().setRuntime("chatgpt")
                useWikiStore.getState().setActiveView("chat")
              }
            }}>{agent === "chatgpt" ? "Open chat" : "Finish"}</Button>
          </footer>
        </>
      )}
    </section>
  )
}
