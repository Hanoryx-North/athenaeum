/**
 * Setting Athenaeum up for the person's coding agents and the Library, from this window.
 *
 * Everything that changes something is a click: connecting Claude Code and Codex, signing in to the
 * Library, syncing. The window runs the command-line tool to do it and shows what it says; it makes
 * no request to the Library itself, and nothing here runs in the background.
 */
import { useEffect, useState } from "react"
import { AlertTriangle, Check, Download, Loader2, PlugZap, RefreshCw, UploadCloud, X } from "lucide-react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { useSetupStore } from "@/stores/setup-store"
import { useWikiStore } from "@/stores/wiki-store"
import { useKnowledgeViewStore } from "@/stores/knowledge-view-store"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"

const DISMISSED = "athenaeum.setup-banner.dismissed"

function useWorkspace(): string | null {
  const project = useWikiStore((s) => s.project)
  return project ? normalizePath(project.path) : null
}

/** What still needs doing, in the words of the checks the CLI reported. */
function usePending() {
  const cli = useSetupStore((s) => s.cli)
  const doctor = useSetupStore((s) => s.doctor)
  const agentsPending = !!doctor?.checks.some((c) => !c.ok && (c.id.startsWith("agent-") || c.id === "integrations"))
  const libraryLinked = !!doctor?.checks.some((c) => c.id === "library" && c.detail.startsWith("linked"))
  return { cliMissing: cli !== null && !cli.found, agentsPending, libraryLinked }
}

export function SetupBanner() {
  const workspace = useWorkspace()
  const check = useSetupStore((s) => s.check)
  const { cliMissing, agentsPending } = usePending()
  const [dismissed, setDismissed] = useState(() => {
    try {
      return sessionStorage.getItem(DISMISSED) === "1"
    } catch {
      return false
    }
  })

  useEffect(() => {
    if (workspace) void check(workspace)
  }, [workspace, check])

  if (dismissed || (!cliMissing && !agentsPending)) return null
  return (
    <p role="status" className="flex items-center gap-2 border-b bg-primary/5 px-4 py-1.5 text-xs">
      <PlugZap className="h-3.5 w-3.5 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1">
        {cliMissing
          ? "Claude Code and Codex can't use this wiki yet: the Athenaeum command-line tool isn't installed."
          : "Your coding agents aren't connected to this wiki yet."}{" "}
        Finish in the <span className="font-medium">Library</span> tab.
      </span>
      <Button variant="ghost" size="sm" className="shrink-0 text-xs" onClick={() => { useKnowledgeViewStore.getState().setTab("library"); useWikiStore.getState().setActiveView("knowledge") }}>
        Finish setup
      </Button>
      <button
        type="button"
        className="rounded p-0.5 hover:bg-muted"
        aria-label="Dismiss"
        onClick={() => {
          setDismissed(true)
          try {
            sessionStorage.setItem(DISMISSED, "1")
          } catch {
            /* a banner that cannot remember simply comes back next session */
          }
        }}
      >
        <X className="h-3 w-3" aria-hidden />
      </button>
    </p>
  )
}

export function SetupCard() {
  const workspace = useWorkspace()
  const { cli, doctor, busy, progress, deviceCode, error, lastSetup, syncPosition, syncMessage } = useSetupStore()
  const { check, install, setup, sync, syncStatus, cancel } = useSetupStore.getState()
  const { agentsPending, libraryLinked } = usePending()
  const [connectLibrary, setConnectLibrary] = useState(false)
  const [autoSync, setAutoSync] = useState(false)
  const [configuring, setConfiguring] = useState(true)
  const [step, setStep] = useState<string | null>(null)
  const [agentMessage, setAgentMessage] = useState<string | null>(null)
  const [externalAgent, setExternalAgent] = useState("codex")

  useEffect(() => {
    try {
      setConfiguring(localStorage.getItem(`athenaeum.setup.route:${workspace}`) === "athenaeum" || !localStorage.getItem(`athenaeum.setup.complete:${workspace}`))
    } catch {
      setConfiguring(true)
    }
    setStep(null)
  }, [workspace])

  useEffect(() => {
    if (workspace && !useSetupStore.getState().cli) void check(workspace)
  }, [workspace, check])
  useEffect(() => {
    if (workspace && libraryLinked) void syncStatus(workspace)
  }, [workspace, libraryLinked, syncStatus])

  if (!workspace) return null
  const working = busy !== null || step !== null

  const configure = async () => {
    setStep("Installing your tools…")
    try {
      const toolsMissing = !cli?.found || doctor?.checks.some((c) => c.id === "integrations" && !c.ok)
      if (toolsMissing && !await install(null)) return
      if (normalizePath(useWikiStore.getState().project?.path ?? "") !== workspace) return
      setStep("Setting up this project's wiki…")
      const ok = await setup(workspace, {
        job: "setup", capture: "instruct",
        ...(externalAgent !== "detected" ? { agents: [externalAgent] } : {}),
        ...(connectLibrary && !libraryLinked ? { library: "" } : {}),
        autoSync: (externalAgent === "claude" || externalAgent === "detected") && autoSync && (connectLibrary || libraryLinked),
      })
      if (ok && useSetupStore.getState().lastSetup?.steps.some((s) => s.id === "agents" && s.status === "skipped")) {
        useSetupStore.setState({ error: "No supported coding agent was found. Install Codex, Claude Code or Cursor, then retry setup." })
        return
      }
      if (ok) {
        setConfiguring(false)
        try {
          localStorage.setItem(`athenaeum.setup.complete:${workspace}`, "1")
          localStorage.setItem(`athenaeum.setup.route:${workspace}`, "external")
        } catch { /* setup still succeeded */ }
      }
    } finally {
      setStep(null)
    }
  }

  return (
    <section className="rounded-md border p-3 text-sm" aria-label="Set up Athenaeum">
      <div className="flex items-center justify-between gap-2">
        <h3 className="flex items-center gap-2 font-medium">
          <PlugZap className="h-4 w-4" aria-hidden />
          Set up your wiki
        </h3>
        <Button variant="ghost" size="sm" className="h-7 text-xs" disabled={working} onClick={() => void check(workspace)}>
          {busy === "check" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <RefreshCw className="mr-1 h-3 w-3" aria-hidden />}
          Check
        </Button>
      </div>

      {cli === null && busy === "check" && <p className="mt-2 text-xs text-muted-foreground">Looking for the command-line tool…</p>}

      <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
        Connect your existing coding agent to this project’s wiki. Codex uses its current session to recall relevant knowledge
        and propose useful findings. No additional API key or subscription is needed for wiki memory.
      </p>

      {configuring ? (
        <div className="mt-4 space-y-4">
          <label className="block text-xs">Coding agent
            <select className="ml-2 rounded border bg-background p-1" value={externalAgent} disabled={working} onChange={(event) => setExternalAgent(event.target.value)}>
              <option value="codex">Codex</option><option value="claude">Claude Code</option><option value="cursor">Cursor</option><option value="detected">All detected agents</option>
            </select>
          </label>
          <p className="text-xs leading-relaxed text-muted-foreground">
            {externalAgent === "codex" || externalAgent === "detected"
              ? "Setup adds wiki tools and Codex instructions to recall before each task and record durable findings before finishing. Review proposals here before they become accepted wiki pages."
              : externalAgent === "claude"
                ? "Setup connects wiki tools and Claude Code hooks for recall and knowledge proposals. Review proposals here before they become accepted wiki pages."
                : "Setup connects wiki tools to Cursor. Ask Cursor to recall project context and propose useful findings; automatic memory instructions are not installed for Cursor."}
          </p>
          <fieldset disabled={working} className="space-y-2 border-t pt-3 text-xs">
            <legend className="sr-only">Library connection</legend>
            {!libraryLinked && (
              <label className="flex cursor-pointer items-start gap-2">
                <input className="mt-0.5 accent-primary" type="checkbox" checked={connectLibrary} onChange={(event) => setConnectLibrary(event.target.checked)} />
                <span>Connect Athenaeum Library<span className="mt-1 block text-muted-foreground">Sign in through your browser and link a private wiki repository. You can also do this later.</span></span>
              </label>
            )}
            {(externalAgent === "claude" || externalAgent === "detected") && (connectLibrary || libraryLinked) && (
              <label className="flex cursor-pointer items-start gap-2">
                <input className="mt-0.5 accent-primary" type="checkbox" checked={autoSync} onChange={(event) => setAutoSync(event.target.checked)} />
                <span>Sync shared, accepted pages after Claude Code turns<span className="mt-1 block text-muted-foreground">Optional. Codex and Cursor use the Sync button. Sync stops at conflicts.</span></span>
              </label>
            )}
          </fieldset>
          <div className="flex flex-wrap items-center gap-2">
            <Button size="sm" disabled={working || cli === null} onClick={() => void configure()}>
              {working ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <Download className="mr-1 h-3 w-3" aria-hidden />}
              {step ?? (externalAgent === "codex" ? "Set up Codex" : externalAgent === "claude" ? "Set up Claude Code" : externalAgent === "cursor" ? "Set up Cursor" : "Set up coding agents")}
            </Button>
            {!working && <Button variant="ghost" size="sm" onClick={() => setConfiguring(false)}>Later</Button>}
            {working && busy !== "check" && <Button variant="ghost" size="sm" onClick={() => void cancel()}>Cancel setup</Button>}
          </div>
          <p className="text-xs text-muted-foreground">Reopen your coding agent after setup to load the connection. Codex follows memory instructions; there is no enforced turn hook.</p>
        </div>
      ) : (
        <div className="mt-3 text-xs">
          {lastSetup?.ok && <p role="status">Your wiki is ready. Reopen your coding agent in this project, then review its wiki proposals in Athenaeum.</p>}
          <Button variant="outline" size="sm" className="mt-2" disabled={working} onClick={() => setConfiguring(true)}>Configure coding agents</Button>
        </div>
      )}
      {cli?.found && !configuring && <div className="mt-2 flex flex-wrap gap-2">
        {(["codex", "claude", "cursor"] as const).map((agent) => <Button key={agent} variant="ghost" size="sm" disabled={working} onClick={() => void (async () => {
          const result = await useSetupStore.getState().operate(workspace, { job: "disconnect_agents", agents: [agent] })
          if (result?.exitCode === 0) { setAgentMessage(`${agent} disconnected. Other agent settings are preserved.`); await check(workspace) }
        })()}>Disconnect {agent === "claude" ? "Claude Code" : agent === "codex" ? "Codex" : "Cursor"}</Button>)}
      </div>}
      {agentMessage && <p role="status" className="mt-2 text-xs">{agentMessage}</p>}

      {doctor && (
        <ul className="mt-2 flex flex-col gap-1 text-xs">
          {doctor.checks.map((c) => (
            <li key={c.id} className="flex items-start gap-2">
              {c.ok ? (
                <Check className="mt-0.5 h-3 w-3 shrink-0 text-emerald-600" aria-label="ok" />
              ) : (
                <AlertTriangle className="mt-0.5 h-3 w-3 shrink-0 text-amber-600" aria-label="needs attention" />
              )}
              <span className="min-w-0">
                <span className="font-medium">{c.id}</span> <span className="break-all text-muted-foreground">{c.detail}</span>
                {!c.ok && c.fix && <span className="block text-muted-foreground">→ {c.fix}</span>}
              </span>
            </li>
          ))}
        </ul>
      )}

      {cli?.found && !configuring && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {agentsPending && (
            <Button size="sm" className="h-7 text-xs" disabled={working} onClick={() => void setup(workspace, { job: "setup", agents: ["codex"], capture: "instruct" })}>
              {busy === "setup" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <PlugZap className="mr-1 h-3 w-3" aria-hidden />}
              Set up Codex
            </Button>
          )}
          {!libraryLinked && (
            <Button
              variant="outline"
              size="sm"
              className="h-7 text-xs"
              disabled={working}
              onClick={() => void setup(workspace, { job: "setup", library: "" })}
            >
              <UploadCloud className="mr-1 h-3 w-3" aria-hidden />
              Connect to the Library
            </Button>
          )}
          {libraryLinked && (
            <Button size="sm" className="h-7 text-xs" disabled={working} onClick={() => void sync(workspace)}>
              {busy === "sync" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <UploadCloud className="mr-1 h-3 w-3" aria-hidden />}
              Sync accepted pages now
            </Button>
          )}
          {working && busy !== "check" && (
            <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => void cancel()}>
              Cancel
            </Button>
          )}
        </div>
      )}

      {deviceCode && (
        <div role="status" className="mt-3 rounded-md border bg-muted/40 p-3 text-xs">
          <p className="font-medium">Approve this sign-in in your browser</p>
          <p className="mt-1 text-muted-foreground">
            Confirm the code <code className="rounded bg-background px-1.5 py-0.5 font-mono text-sm text-foreground">{deviceCode.code}</code>
          </p>
          <Button variant="outline" size="sm" className="mt-2 h-7 text-xs" onClick={() => void openUrl(deviceCode.url).catch(() => undefined)}>
            Open the Library sign-in
          </Button>
          <p className="mt-2 break-all text-muted-foreground">{deviceCode.url}</p>
        </div>
      )}

      {libraryLinked && (syncPosition || syncMessage) && (
        <p className="mt-2 text-xs text-muted-foreground" role="status">
          {syncPosition && <span className={cn("mr-2 rounded-full border px-2 py-0.5")}>{syncPosition.replace("_", " ")}</span>}
          {syncMessage}
        </p>
      )}

      {lastSetup && (
        <ul className="mt-2 flex flex-col gap-1 text-xs">
          {lastSetup.steps
            .filter((step) => step.status === "failed")
            .map((step) => (
              <li key={step.id} role="alert" className="text-destructive">
                {step.id}: {step.detail}
                {step.fix && <span className="block text-muted-foreground">→ {step.fix}</span>}
              </li>
            ))}
        </ul>
      )}

      {error && (
        <p role="alert" className="mt-2 text-xs text-destructive">
          {error}
        </p>
      )}

      {progress.length > 0 && (
        <pre className="mt-2 max-h-32 overflow-y-auto rounded bg-muted p-2 font-mono text-[11px] whitespace-pre-wrap" aria-label="Progress">
          {progress.slice(-12).join("\n")}
        </pre>
      )}
    </section>
  )
}
