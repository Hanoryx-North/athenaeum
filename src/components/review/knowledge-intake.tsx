import { useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"

export function KnowledgeIntake({ workspace }: { workspace: string }) {
  const [title, setTitle] = useState("")
  const [text, setText] = useState("")
  const [scope, setScope] = useState("project")
  const [core, setCore] = useState(false)
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState("")
  const [error, setError] = useState("")
  const bytes = new TextEncoder().encode(text).length
  const submit = async () => {
    setBusy(true); setError(""); setMessage("")
    try {
      const result = await invoke<{ status: string }>("background_memory", {
        projectPath: workspace, action: "ingest", document: { title, text, scope, core, wikiRoot: workspace },
      })
      setMessage(result.status === "already_queued" ? "This source is already queued or processed." : "Queued. Track progress in Coding agents → Background memory.")
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setBusy(false) }
  }
  return <details className="border-b px-4 py-3">
    <summary className="cursor-pointer text-sm font-medium">Extract knowledge from text</summary>
    <div className="mt-3 space-y-3">
      <p className="text-xs text-muted-foreground">Extract lessons with your configured memory model.</p>
      <div className="space-y-1"><Label htmlFor="intake-title">Source title</Label><Input id="intake-title" value={title} onChange={(event) => setTitle(event.target.value)} maxLength={200} disabled={busy} /></div>
      <div className="space-y-1"><Label htmlFor="intake-text">Source text</Label><textarea id="intake-text" className="min-h-40 w-full rounded-md border bg-background p-2 text-sm" value={text} onChange={(event) => setText(event.target.value)} disabled={busy} /></div>
      <div className="flex flex-wrap items-center gap-3 text-sm">
        <Label htmlFor="intake-scope">Applies to</Label>
        <select id="intake-scope" className="rounded border bg-background p-1" value={scope} onChange={(event) => setScope(event.target.value)} disabled={busy}>
          <option value="project">This project</option><option value="universal">All projects</option>
        </select>
        <span className="text-xs text-muted-foreground">{bytes.toLocaleString()} / 65,536 bytes</span>
        <Button size="sm" disabled={busy || !title.trim() || !text.trim() || bytes > 65536} onClick={() => void submit()}>{busy ? "Queueing…" : "Extract lessons"}</Button>
      </div>
      <label className="flex items-center gap-2 text-sm" htmlFor="intake-core">
        <input id="intake-core" type="checkbox" checked={core} onChange={event => setCore(event.target.checked)} disabled={busy} className="accent-primary focus-visible:outline-2 focus-visible:outline-ring" />
        Core guidance
        <span className="text-xs text-muted-foreground">Include in matching task summaries</span>
      </label>
      {message && <p role="status" className="text-sm">{message}</p>}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    </div>
  </details>
}
