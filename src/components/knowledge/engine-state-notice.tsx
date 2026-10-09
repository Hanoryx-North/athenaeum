/**
 * Where the app admits what it does not know.
 *
 * Every knowledge surface can be in a state where showing nothing is the
 * honest answer: no vault here, an index that has never been built or is
 * behind the vault, an engine speaking another protocol, or a shell that
 * cannot reach the engine at all. Each one says what it is, why, and offers
 * the action that actually helps — never a spinner that never ends and never
 * an empty list pretending to be an answer.
 */
import type { ReactNode } from "react"
import { AlertTriangle, Ban, Database, FileWarning, PlugZap, RefreshCw, Wrench } from "lucide-react"
import { Button } from "@/components/ui/button"
import { engineStateLabel, type EngineProductState } from "@/lib/engine-state"
import { cn } from "@/lib/utils"

export function EngineStateNotice({
  state,
  onRetry,
  onRebuildIndex,
  rebuilding,
  onOpenProjectSettings,
  className,
}: {
  state: EngineProductState
  onRetry: () => void
  onRebuildIndex: () => void
  rebuilding: boolean
  /**
   * What to do when the project has no vault yet. The desktop does not create
   * one behind the person's back; this opens the folder so the CLI or the
   * project dialog can.
   */
  onOpenProjectSettings?: () => void
  className?: string
}) {
  if (state.kind === "ready" || state.kind === "loading") return null

  const { Icon, tone, title, detail, actions } = describe(state, {
    onRetry,
    onRebuildIndex,
    rebuilding,
    onOpenProjectSettings,
  })

  return (
    <div
      role="status"
      aria-live="polite"
      className={cn(
        "flex flex-wrap items-start gap-3 border-b px-4 py-3 text-sm",
        tone,
        className,
      )}
    >
      <Icon className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="font-medium">{title}</p>
        <p className="mt-0.5 text-xs text-muted-foreground">{detail}</p>
        {state.kind === "stale-index" && state.indexedAt && (
          <p className="mt-0.5 text-xs text-muted-foreground">
            Last indexed {new Date(state.indexedAt).toLocaleString()}.
          </p>
        )}
        {state.kind === "engine-error" && (
          <p className="mt-0.5 font-mono text-xs text-muted-foreground">{state.code}</p>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2">{actions}</div>
    </div>
  )
}

function describe(
  state: Exclude<EngineProductState, { kind: "ready" } | { kind: "loading" }>,
  handlers: {
    onRetry: () => void
    onRebuildIndex: () => void
    rebuilding: boolean
    onOpenProjectSettings?: () => void
  },
): { Icon: typeof AlertTriangle; tone: string; title: string; detail: string; actions: ReactNode } {
  const retry = (
    <Button key="retry" variant="outline" size="sm" className="h-7 text-xs" onClick={handlers.onRetry}>
      <RefreshCw className="mr-1 h-3 w-3" aria-hidden />
      Check again
    </Button>
  )
  const rebuild = (
    <Button
      key="rebuild"
      size="sm"
      className="h-7 text-xs"
      disabled={handlers.rebuilding}
      onClick={handlers.onRebuildIndex}
    >
      <Wrench className="mr-1 h-3 w-3" aria-hidden />
      {handlers.rebuilding ? "Rebuilding…" : "Rebuild the index"}
    </Button>
  )

  switch (state.kind) {
    case "absent-vault":
      return {
        Icon: FileWarning,
        tone: "border-amber-500/40 bg-amber-500/5",
        title: engineStateLabel(state),
        detail: `${state.message} Knowledge surfaces read the vault through the engine; with no vault there is nothing for them to read. Create one with the project dialog, or run \`athenaeum init\` in the project folder.`,
        actions: (
          <>
            {handlers.onOpenProjectSettings && (
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                onClick={handlers.onOpenProjectSettings}
              >
                Open the project folder
              </Button>
            )}
            {retry}
          </>
        ),
      }
    case "never-indexed":
      return {
        Icon: Database,
        tone: "border-border bg-muted/40",
        title: engineStateLabel(state),
        detail: `${state.message} An empty reading here would be a lie: the numbers are missing, not zero.`,
        actions: rebuild,
      }
    case "stale-index":
      return {
        Icon: Database,
        tone: "border-amber-500/40 bg-amber-500/5",
        title: engineStateLabel(state),
        detail: state.message,
        actions: rebuild,
      }
    case "protocol-mismatch":
      return {
        Icon: Ban,
        tone: "border-destructive/40 bg-destructive/5",
        title: engineStateLabel(state),
        detail: `${state.message} Update the app or the engine so both speak the same version; nothing is shown from an answer this window cannot read.`,
        actions: retry,
      }
    case "unreachable":
      return {
        Icon: PlugZap,
        tone: "border-destructive/40 bg-destructive/5",
        title: engineStateLabel(state),
        detail: `${state.message} The engine runs in this process; if it cannot be reached, the app needs to be restarted.`,
        actions: retry,
      }
    case "engine-error":
      return {
        Icon: AlertTriangle,
        tone: "border-destructive/40 bg-destructive/5",
        title: engineStateLabel(state),
        detail: state.message,
        actions: retry,
      }
  }
}
