import { useState, type FormEvent } from "react"
import { Search, ChevronDown, ChevronRight } from "lucide-react"
import type { BridgeBrief } from "@athenaeum/protocol/bridge"
import { memoryBrief } from "@/lib/core-bridge"

/** A concise view of the local core's graph memory for a later task. */
export function MemoryBriefPanel({ workspace, openPage }: { workspace: string; openPage: (path: string) => void }) {
  const [expanded, setExpanded] = useState(false)
  const [task, setTask] = useState("")
  const [brief, setBrief] = useState<BridgeBrief | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")

  async function retrieve(event: FormEvent) {
    event.preventDefault()
    if (!task.trim()) return
    setBusy(true)
    setError("")
    try {
      const context = await memoryBrief(workspace, task.trim())
      setBrief(context.brief)
    } catch (cause) {
      setBrief(null)
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  return <section className="mb-2 border-b pb-2" aria-label="Local memory">
    <button className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-sm hover:bg-accent/50" onClick={() => setExpanded(value => !value)} aria-expanded={expanded}>
      {expanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
      <Search className="h-3.5 w-3.5 text-emerald-600" />
      <span className="flex-1 text-left font-medium">Local memory</span>
    </button>
    {expanded && <div className="px-2 pb-2">
      <form onSubmit={retrieve} className="mt-2 flex gap-1">
        <input className="min-w-0 flex-1 rounded border bg-background px-2 py-1 text-xs" value={task} onChange={event => setTask(event.target.value)} placeholder="What do I need to know?" aria-label="Task to retrieve knowledge for" />
        <button type="submit" className="rounded bg-primary px-2 text-xs text-primary-foreground disabled:opacity-50" disabled={busy || !task.trim()}>{busy ? "…" : "Find"}</button>
      </form>
      {error && <p role="alert" className="mt-2 text-xs text-destructive">{error}</p>}
      {brief && <div className="mt-3 space-y-2" aria-live="polite">
        {brief.nodes.length === 0 && <p className="text-xs text-muted-foreground">No relevant knowledge yet. Add wiki pages with source evidence to build memory.</p>}
        {brief.nodes.map(node => <article key={node.id} className="rounded-md bg-muted/50 p-2 text-xs">
          <div className="font-medium">{node.title}</div>
          <p className="mt-1 text-muted-foreground">{node.summary}</p>
          <div className="mt-1 text-muted-foreground">{node.authority} · {node.status}</div>
          {node.page && <button className="mt-1 text-left text-primary underline-offset-2 hover:underline" onClick={() => openPage(`${workspace.replace(/\/+$/, "")}/${node.page!.path}`)}>{node.page.path}</button>}
        </article>)}
        {brief.anchors.length > 0 && <div className="text-xs text-muted-foreground"><strong>Evidence</strong><ul className="mt-1 list-inside list-disc break-all">{brief.anchors.map(anchor => <li key={anchor}>{anchor}</li>)}</ul></div>}
        {brief.warnings.length > 0 && <div className="text-xs text-amber-700"><strong>Review</strong><ul className="mt-1 list-inside list-disc">{brief.warnings.map(warning => <li key={warning}>{warning}</li>)}</ul></div>}
      </div>}
    </div>}
  </section>
}
