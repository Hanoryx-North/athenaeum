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
  const { cliMissing, agentsPending, libraryLinked } = usePending()

  useEffect(() => {
    if (workspace && !useSetupStore.getState().cli) void check(workspace)
  }, [workspace, check])
  useEffect(() => {
    if (workspace && libraryLinked) void syncStatus(workspace)
  }, [workspace, libraryLinked, syncStatus])

  if (!workspace) return null
  const working = busy !== null

  return (
    <section className="rounded-md border p-3 text-sm" aria-label="Set up Athenaeum">
      <div className="flex items-center justify-between gap-2">
        <h3 className="flex items-center gap-2 font-medium">
          <PlugZap className="h-4 w-4" aria-hidden />
          Coding agents and Library
        </h3>
        <Button variant="ghost" size="sm" className="h-7 text-xs" disabled={working} onClick={() => void check(workspace)}>
          {busy === "check" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <RefreshCw className="mr-1 h-3 w-3" aria-hidden />}
          Check
        </Button>
      </div>

      {cli === null && busy === "check" && <p className="mt-2 text-xs text-muted-foreground">Looking for the command-line tool…</p>}

      {cliMissing && (
        <div className="mt-2 text-xs">
          <p className="text-muted-foreground">
            The command-line tool is what lets Claude Code, Codex and Cursor use this wiki, and what syncs it with the Library. It
            installs for your user only, with no administrator prompt.
          </p>
          <Button size="sm" className="mt-2 h-7 text-xs" disabled={working} onClick={() => void install(workspace)}>
            {busy === "install" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <Download className="mr-1 h-3 w-3" aria-hidden />}
            Install the command-line tool
          </Button>
        </div>
      )}

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

      {cli?.found && (
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {agentsPending && (
            <Button size="sm" className="h-7 text-xs" disabled={working} onClick={() => void setup(workspace, { job: "setup" })}>
              {busy === "setup" ? <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden /> : <PlugZap className="mr-1 h-3 w-3" aria-hidden />}
              Connect my coding agents
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
