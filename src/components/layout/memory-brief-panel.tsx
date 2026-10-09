import { useCallback, useEffect, useState, type FormEvent } from "react"
import { Search, ChevronDown, ChevronRight } from "lucide-react"
import type { BridgeBrief, BridgeProposal } from "@athenaeum/protocol/bridge"
import { acceptProposal, memoryBrief, memoryProposals, rejectProposal } from "@/lib/core-bridge"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"

function messageOf(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause)
}

/** A concise view of the local core's graph memory for a later task. */
export function MemoryBriefPanel({ workspace, openPage }: { workspace: string; openPage: (path: string) => void }) {
  const [expanded, setExpanded] = useState(false)
  const [task, setTask] = useState("")
  const [brief, setBrief] = useState<BridgeBrief | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState("")
  const [proposals, setProposals] = useState<BridgeProposal[]>([])
  const [proposalError, setProposalError] = useState("")
  const [reviewing, setReviewing] = useState<string | null>(null)
  const [saved, setSaved] = useState("")

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
      setError(messageOf(cause))
    } finally {
      setBusy(false)
    }
  }

  // What an agent proposed through the bridge waits here for a person; nothing it proposes is a page yet.
  const loadProposals = useCallback(async () => {
    try {
      setProposals(await memoryProposals(workspace))
      setProposalError("")
    } catch (cause) {
      setProposals([])
      // A project with no knowledge workspace has nothing proposed; that is not a fault.
      const message = messageOf(cause)
      setProposalError(message.startsWith("workspace_not_found") ? "" : message)
    }
  }, [workspace])

  useEffect(() => {
    if (expanded) void loadProposals()
  }, [expanded, loadProposals])

  async function review(node: string, action: "accept" | "reject") {
    setReviewing(node)
    setProposalError("")
    setSaved("")
    try {
      if (action === "accept") {
        const accepted = await acceptProposal(workspace, node)
        setSaved(accepted.page.path)
        // The accepted page is new on disk; let the tree and everything watching it see it.
        await refreshProjectFileTree(workspace, { bumpDataVersion: true }).catch(() => undefined)
      } else {
        await rejectProposal(workspace, node)
      }
      await loadProposals()
    } catch (cause) {
      setProposalError(messageOf(cause))
    } finally {
      setReviewing(null)
    }
  }

  return <section className="mb-2 border-b pb-2" aria-label="Local memory">
    <button className="flex w-full items-center gap-1.5 rounded-md px-2 py-1.5 text-sm hover:bg-accent/50" onClick={() => setExpanded(value => !value)} aria-expanded={expanded}>
      {expanded ? <ChevronDown className="h-3.5 w-3.5" /> : <ChevronRight className="h-3.5 w-3.5" />}
      <Search className="h-3.5 w-3.5 text-emerald-600" />
      <span className="flex-1 text-left font-medium">Local memory</span>
      {proposals.length > 0 && <span className="rounded-full bg-primary px-1.5 text-[10px] text-primary-foreground" aria-label={`${proposals.length} proposals to review`}>{proposals.length}</span>}
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
      {proposalError && <p role="alert" className="mt-3 text-xs text-destructive">{proposalError}</p>}
      {saved && <p role="status" className="mt-3 text-xs text-muted-foreground">Saved as <button className="text-primary underline-offset-2 hover:underline" onClick={() => openPage(`${workspace.replace(/\/+$/, "")}/${saved}`)}>{saved}</button>. It stays unverified until a source confirms it.</p>}
      {proposals.length > 0 && <div className="mt-3 space-y-2" aria-label="Proposed by agents">
        <div className="text-xs font-medium">Proposed by agents</div>
        {proposals.map(({ node, evidence }) => <article key={node.id} className="rounded-md border p-2 text-xs">
          <div className="font-medium">{node.title}</div>
          <p className="mt-1 text-muted-foreground">{node.summary}</p>
          <div className="mt-1 text-muted-foreground">{node.provenance.source}{node.provenance.detail ? `: ${node.provenance.detail}` : ""} · {node.type}</div>
          {evidence.length > 0
            ? <ul className="mt-1 list-inside list-disc break-all text-muted-foreground">{evidence.map(item => <li key={item.id}>{item.anchor?.file ?? item.kind}</li>)}</ul>
            : <p className="mt-1 text-amber-700">No source evidence yet, so it cannot be accepted.</p>}
          <div className="mt-2 flex gap-1">
            <button className="rounded bg-primary px-2 py-0.5 text-primary-foreground disabled:opacity-50" disabled={reviewing !== null || evidence.length === 0} onClick={() => void review(node.id, "accept")}>{reviewing === node.id ? "…" : "Accept"}</button>
            <button className="rounded border px-2 py-0.5 disabled:opacity-50" disabled={reviewing !== null} onClick={() => void review(node.id, "reject")}>Reject</button>
          </div>
        </article>)}
      </div>}
    </div>}
  </section>
}
