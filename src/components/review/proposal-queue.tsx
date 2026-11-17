import { useEffect } from "react"
import { RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useProposalStore } from "@/stores/proposal-store"
import { useAutomationStore } from "@/stores/automation-store"
import { anchorLabel } from "@/lib/engine-present"
import { KnowledgeIntake } from "./knowledge-intake"

export function ProposalQueue() {
  const workspace = useWikiStore(s => s.project?.path)
  const state = useProposalStore()
  const engine = useEngineStore(s => s.state)
  const savingError = useAutomationStore(s => s.error)
  useEffect(() => {
    if (!workspace) return
    void state.load()
    const timer = setInterval(() => { if (document.visibilityState === "visible") void state.load() }, 15_000)
    return () => clearInterval(timer)
  }, [workspace, state.load])

  return <div className="flex h-full flex-col">
    <header className="flex items-center justify-between gap-3 border-b px-4 py-3">
      <h2 className="text-sm font-semibold">Pending wiki pages{state.proposals.length ? ` (${state.proposals.length})` : ""}</h2>
      <Button variant="ghost" size="sm" disabled={state.loading || !workspace} onClick={() => void state.load({ force: true })}>
        <RefreshCw className={state.loading ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />Refresh
      </Button>
    </header>
    {workspace && <KnowledgeIntake key={workspace} workspace={workspace} />}
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      {(state.error || savingError) && <p role="alert" className="mb-4 text-sm text-destructive">{state.error || savingError}</p>}
      {engine.kind !== "ready" ? <p role="status" className="text-sm text-muted-foreground">{engine.kind === "loading" ? "Loading…" : engine.message}</p> :
        state.proposals.length === 0 ? <p role="status" className="text-sm text-muted-foreground">{state.loading ? "Loading…" : state.error || savingError ? "Refresh to try again." : "All pages saved."}</p> :
        <ul className="divide-y">
          {state.proposals.map(({ node, evidence }) => <li key={node.id} className="space-y-3 py-4 first:pt-0">
            <div className="flex items-start justify-between gap-4">
              <div className="min-w-0 space-y-1">
                <h3 className="break-words text-sm font-medium">{node.title}</h3>
                <p className="text-xs text-muted-foreground">{node.provenance.detail === "athenaeum-background-memory" && evidence.length === 0 ? "Waiting for source evidence" : "Saving automatically"}</p>
              </div>
              <Button size="sm" variant="ghost" disabled={state.busy[node.id]} onClick={() => void state.reject(node.id)}>{state.busy[node.id] ? "Discarding…" : "Discard"}</Button>
            </div>
            <p className="whitespace-pre-wrap break-words text-sm">{node.summary}</p>
            {state.failures[node.id] && <p role="alert" className="text-sm text-destructive">{state.failures[node.id]}</p>}
            {(node.content || evidence.length > 0) && <details className="text-sm">
              <summary className="cursor-pointer text-muted-foreground">Details</summary>
              <div className="mt-3 space-y-3">
                {node.content && <p className="whitespace-pre-wrap break-words">{node.content}</p>}
                {evidence.length > 0 && <ul className="space-y-2 text-xs text-muted-foreground">{evidence.map(item => <li key={item.id}>{anchorLabel(item)}{item.excerpt && <blockquote className="mt-1 whitespace-pre-wrap">{item.excerpt}</blockquote>}</li>)}</ul>}
              </div>
            </details>}
          </li>)}
        </ul>}
    </div>
  </div>
}
