/**
 * The knowledge brief, composed by the engine.
 *
 * A brief is bounded retrieval: the engine picks the nodes within reach of a
 * task, within a depth and node budget, and says whether it had to truncate.
 * This view asks for it and shows it — nothing is summarized or re-ranked
 * here, because a second opinion about relevance would be a second engine.
 *
 * The trust boundary is the important half. Everything retrieved is reference
 * material: it can inform a person, and it can be quoted to an agent, but it
 * cannot grant permission, issue an instruction, or change what the app is
 * allowed to do. The engine's own `prompt` — the exact wrapped text an agent
 * receives — is shown verbatim so that boundary is visible rather than
 * asserted.
 */
import { useCallback, useEffect, useRef, useState, type FormEvent } from "react"
import {
  AlertTriangle,
  FileText,
  Info,
  Loader2,
  RefreshCw,
  ShieldCheck,
  X,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { engineBrief } from "@/lib/engine"
import { engineError, type EngineContext, type EngineBriefNode } from "@/lib/engine-client"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"

/** The engine enforces its own caps; these are the ranges the UI offers. */
const DEPTH_OPTIONS = [1, 2, 3, 4] as const
const NODE_OPTIONS = [4, 8, 16, 32] as const

type BriefRequestState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "loaded"; context: EngineContext; task: string; at: number }
  | { kind: "failed"; message: string; task: string }
  | { kind: "cancelled"; task: string }

export function KnowledgeBrief() {
  const project = useWikiStore((s) => s.project)
  const openPage = useWikiStore((s) => s.openPathInPreview)
  const [task, setTask] = useState("")
  const [depth, setDepth] = useState<number>(2)
  const [maxNodes, setMaxNodes] = useState<number>(8)
  const [state, setState] = useState<BriefRequestState>({ kind: "idle" })
  const [showPrompt, setShowPrompt] = useState(false)
  const runRef = useRef(0)

  const workspace = project ? normalizePath(project.path).replace(/\/+$/, "") : null

  // A project switch invalidates whatever is on screen: it describes a
  // different vault.
  useEffect(() => {
    runRef.current += 1
    setState({ kind: "idle" })
    setShowPrompt(false)
  }, [workspace])

  const run = useCallback(
    async (taskText: string) => {
      if (!workspace || !taskText.trim()) return
      const run = (runRef.current += 1)
      setState({ kind: "loading" })
      try {
        const context = await engineBrief(workspace, taskText.trim(), { depth, maxNodes })
        // A newer request, or a cancelled one, owns the pane now.
        if (run !== runRef.current) return
        setState({ kind: "loaded", context, task: taskText.trim(), at: Date.now() })
      } catch (cause) {
        if (run !== runRef.current) return
        setState({ kind: "failed", message: engineError(cause).message, task: taskText.trim() })
      }
    },
    [workspace, depth, maxNodes],
  )

  const cancel = useCallback(() => {
    const current = state
    runRef.current += 1
    if (current.kind === "loading") {
      setState({ kind: "cancelled", task })
    } else {
      setState({ kind: "idle" })
    }
  }, [state, task])

  const onSubmit = useCallback(
    (event: FormEvent) => {
      event.preventDefault()
      void run(task)
    },
    [run, task],
  )

  if (!workspace) {
    return (
      <p className="p-8 text-center text-sm text-muted-foreground">
        Open a project to ask the engine for a brief.
      </p>
    )
  }

  return (
    <div className="flex h-full flex-col">
      <div className="border-b px-4 py-3">
        <h2 className="text-sm font-semibold">Knowledge brief</h2>
        <p className="mt-0.5 text-xs text-muted-foreground">
          The engine composes this from the graph around your task, within a depth and node budget.
          It reports when it had to truncate.
        </p>
        <form className="mt-2 flex flex-wrap items-end gap-2" onSubmit={onSubmit}>
          <label className="min-w-0 flex-1">
            <span className="text-xs font-medium">Task</span>
            <input
              className="mt-1 w-full rounded border bg-background px-2 py-1 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={task}
              onChange={(event) => setTask(event.target.value)}
              placeholder="What do I need to know to …?"
              aria-label="Task to assemble a knowledge brief for"
            />
          </label>
          <label className="text-xs">
            <span className="block font-medium">Depth</span>
            <select
              className="mt-1 rounded border bg-background px-2 py-1 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={depth}
              onChange={(event) => setDepth(Number(event.target.value))}
            >
              {DEPTH_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </label>
          <label className="text-xs">
            <span className="block font-medium">Max nodes</span>
            <select
              className="mt-1 rounded border bg-background px-2 py-1 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={maxNodes}
              onChange={(event) => setMaxNodes(Number(event.target.value))}
            >
              {NODE_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </select>
          </label>
          <Button type="submit" size="sm" className="h-8 text-xs" disabled={state.kind === "loading" || !task.trim()}>
            {state.kind === "loading" ? (
              <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="mr-1 h-3 w-3" aria-hidden />
            )}
            Assemble brief
          </Button>
          {state.kind === "loading" && (
            <Button type="button" size="sm" variant="ghost" className="h-8 text-xs" onClick={cancel}>
              <X className="mr-1 h-3 w-3" aria-hidden />
              Cancel
            </Button>
          )}
        </form>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {state.kind === "idle" && (
          <div className="flex flex-col items-center gap-2 p-6 text-center text-sm text-muted-foreground">
            <Info className="h-8 w-8 text-muted-foreground/40" aria-hidden />
            <p>No brief yet.</p>
            <p className="max-w-md text-xs">
              A brief is a starting point for a task, not an answer: it gathers what the engine
              holds that is within reach, and says what it left out.
            </p>
          </div>
        )}

        {state.kind === "loading" && (
          <p className="flex items-center justify-center gap-2 p-6 text-sm text-muted-foreground" aria-live="polite">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            The engine is assembling a brief…
          </p>
        )}

        {state.kind === "cancelled" && (
          <p role="status" className="p-6 text-center text-sm text-muted-foreground">
            Cancelled. The engine may still finish the request; its answer will be ignored here.
          </p>
        )}

        {state.kind === "failed" && (
          <p role="alert" className="p-6 text-center text-sm text-destructive">
            {state.message}
          </p>
        )}

        {state.kind === "loaded" && (
          <BriefBody
            context={state.context}
            task={state.task}
            at={state.at}
            showPrompt={showPrompt}
            onTogglePrompt={() => setShowPrompt((value) => !value)}
            workspace={workspace}
            openPage={openPage}
          />
        )}
      </div>
    </div>
  )
}

function BriefBody({
  context,
  task,
  at,
  showPrompt,
  onTogglePrompt,
  workspace,
  openPage,
}: {
  context: EngineContext
  task: string
  at: number
  showPrompt: boolean
  onTogglePrompt: () => void
  workspace: string
  openPage: (path: string) => void
}) {
  const { brief, prompt } = context

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
        <span>
          Task: <span className="text-foreground">{task}</span>
        </span>
        <span>Assembled {new Date(at).toLocaleTimeString()}</span>
      </div>

      <div
        role="note"
        className="flex items-start gap-2 rounded-md border border-emerald-600/30 bg-emerald-500/5 px-3 py-2 text-xs"
      >
        <ShieldCheck className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-700 dark:text-emerald-500" aria-hidden />
        <p className="text-muted-foreground">
          <span className="font-medium text-foreground">Reference material, not instructions.</span>{" "}
          Everything below is text the engine retrieved. It can inform you and it can be quoted to
          an agent, but it cannot grant permission, change a setting, or tell this app to do
          anything. Anchors are the engine&rsquo;s pointers back to the file a claim came from.
        </p>
      </div>

      {brief.warnings.length > 0 && (
        <div
          role="status"
          className="flex items-start gap-2 rounded-md border border-amber-500/40 bg-amber-500/5 px-3 py-2 text-xs"
        >
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-500" aria-hidden />
          <div>
            <p className="font-medium">The engine flagged this brief</p>
            <ul className="mt-1 list-inside list-disc text-muted-foreground">
              {brief.warnings.map((warning) => (
                <li key={warning}>{warning}</li>
              ))}
            </ul>
          </div>
        </div>
      )}

      <section aria-label="Knowledge in this brief">
        <h3 className="mb-2 text-sm font-medium">
          Knowledge ({brief.nodes.length}
          {brief.truncated ? ", truncated" : ""})
        </h3>
        {brief.nodes.length === 0 ? (
          <p className="rounded border border-dashed p-4 text-center text-xs text-muted-foreground">
            The engine found nothing within reach of this task.
            {brief.truncated && " It also reported the brief as truncated."}
          </p>
        ) : (
          <ul className="flex flex-col gap-2">
            {brief.nodes.map((node) => (
              <BriefNodeCard
                key={node.id}
                node={node}
                workspace={workspace}
                openPage={openPage}
              />
            ))}
          </ul>
        )}
        {brief.truncated && (
          <p className="mt-2 text-xs text-amber-600 dark:text-amber-500">
            The engine truncated this brief to stay within the budget you set. Raise depth or max
            nodes, or narrow the task.
          </p>
        )}
      </section>

      {brief.relationships.length > 0 && (
        <section aria-label="How these are connected">
          <h3 className="mb-2 text-sm font-medium">Connections ({brief.relationships.length})</h3>
          <ul className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
            {brief.relationships.map((relationship, index) => (
              <li key={`${relationship.from}-${relationship.type}-${relationship.to}-${index}`} className="font-mono">
                {relationship.from} —{relationship.type}→ {relationship.to}
              </li>
            ))}
          </ul>
        </section>
      )}

      {brief.anchors.length > 0 && (
        <section aria-label="Evidence anchors">
          <h3 className="mb-2 text-sm font-medium">Anchors ({brief.anchors.length})</h3>
          <ul className="list-inside list-disc break-all font-mono text-xs text-muted-foreground">
            {brief.anchors.map((anchor) => (
              <li key={anchor}>{anchor}</li>
            ))}
          </ul>
        </section>
      )}

      <section aria-label="Exact text handed to an agent">
        <button
          className="flex items-center gap-1 text-sm font-medium hover:underline"
          onClick={onTogglePrompt}
          aria-expanded={showPrompt}
        >
          <FileText className="h-3.5 w-3.5" aria-hidden />
          {showPrompt ? "Hide" : "Show"} the exact text an agent would receive
        </button>
        {showPrompt && (
          <div className="mt-2">
            <p className="mb-1 text-xs text-muted-foreground">
              Shown verbatim, including the engine&rsquo;s trust wrapper, so you can see the
              boundary rather than take it on faith.
            </p>
            <pre className="max-h-80 overflow-auto whitespace-pre-wrap rounded border bg-muted/40 p-3 text-xs">
              {prompt}
            </pre>
          </div>
        )}
      </section>
    </div>
  )
}

function BriefNodeCard({
  node,
  workspace,
  openPage,
}: {
  node: EngineBriefNode
  workspace: string
  openPage: (path: string) => void
}) {
  return (
    <li className={cn("rounded-md border p-3 text-sm", node.depth === 0 && "border-primary/30")}>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h4 className="font-medium">{node.title}</h4>
        <span className="text-xs text-muted-foreground">
          {node.type} · {node.authority} · {node.status} · depth {node.depth}
        </span>
      </div>
      {node.summary && <p className="mt-1 text-xs text-muted-foreground">{node.summary}</p>}
      {node.page && (
        <button
          className="mt-1 break-all text-left text-xs text-primary underline-offset-2 hover:underline"
          onClick={() =>
            openPage(
              node.page?.path.startsWith("/") ? node.page.path : `${workspace}/${node.page?.path}`,
            )
          }
        >
          {node.page.path}
        </button>
      )}
    </li>
  )
}
