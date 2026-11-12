import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { useBackgroundMemoryStore } from "@/stores/background-memory-store"

export function BackgroundMemory({ workspace }: { workspace: string }) {
  const { status, busy, error, refresh, configure, run } = useBackgroundMemoryStore()
  const [model, setModel] = useState("gpt-6-luna")
  useEffect(() => { void refresh(workspace) }, [workspace, refresh])
  useEffect(() => { if (status) setModel(status.settings.model) }, [status?.settings.model])
  const settings = status?.settings ?? { enabled: false, includeCodex: true, model: "gpt-6-luna" }
  const pending = status?.jobs.filter((job) => job.status === "pending").length ?? 0
  const completed = status?.jobs.filter((job) => job.status === "completed").length ?? 0
  const proposed = status?.jobs.reduce((count, job) => count + (job.proposed?.length ?? 0), 0) ?? 0
  const failed = status?.jobs.filter((job) => job.status === "failed").length ?? 0
  return (
    <section className="space-y-3 border-t pt-5" aria-labelledby="background-memory-title">
      <h3 id="background-memory-title" className="font-medium">Background memory</h3>
      <p className="max-w-prose text-sm text-muted-foreground">Learn from completed coding turns with a separate model using your ChatGPT connection. Findings include checked source excerpts and appear in Review and the graph. Your plan's usage limits apply.</p>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={settings.enabled} disabled={busy || !status} onChange={(event) => void configure(workspace, { ...settings, model, enabled: event.target.checked })} />
        Learn in the background
      </label>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" checked={settings.includeCodex} disabled={busy || !status} onChange={(event) => void configure(workspace, { ...settings, model, includeCodex: event.target.checked })} />
        Include connected Codex sessions
      </label>
      <div className="flex max-w-md flex-wrap items-end gap-2">
        <div className="min-w-0 flex-1 space-y-1.5">
          <Label htmlFor="background-memory-model">Memory model</Label>
          <Input id="background-memory-model" value={model} onChange={(event) => setModel(event.target.value)} disabled={busy} />
        </div>
        <Button variant="outline" disabled={busy || !status || !model.trim() || model === settings.model} onClick={() => void configure(workspace, { ...settings, model })}>Save model</Button>
      </div>
      <p className="text-xs text-muted-foreground">Memory activity stays out of chat. The selected model is checked before enabling. It never falls back to the coding model.</p>
      <p role="status" className="text-sm">{!status ? busy ? "Checking memory service..." : "Memory service unavailable." : !settings.enabled ? "Background learning is off." : status.active === false ? "Automatic capture is off in your agent connection settings." : `${status.state === "running" ? "Processing. " : "Ready. "}${pending} queued, ${completed} completed, ${proposed} proposed${failed ? `, ${failed} failed` : ""}.`}</p>
      {settings.wikiRoot && <p className="break-all text-xs text-muted-foreground">Memory wiki: {settings.wikiRoot}</p>}
      {(error || status?.error || failed > 0) && <p role="alert" className="text-sm text-destructive">{error || status?.error || "Some memory jobs failed. Check the model and connection before trying again. Completed jobs remain available."}</p>}
      <div className="flex flex-wrap gap-2">
        <Button variant="ghost" size="sm" disabled={busy || !settings.enabled} onClick={() => void run(workspace)}>{busy ? "Checking..." : "Process pending memory"}</Button>
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => void refresh(workspace)}>Check memory service</Button>
      </div>
      {!!status?.jobs.length && <ul className="space-y-1 text-xs text-muted-foreground" aria-label="Recent memory jobs">{status.jobs.slice(0, 3).map((job) => <li key={job.id}>{job.source === "codex" ? "Codex" : "Athenaeum"}: {job.status === "failed" ? `failed during ${job.failureStage ?? "processing"}` : job.proposed?.length ? `${job.proposed.length} proposed` : job.outcome === "no_candidates" ? "no reusable finding" : job.outcome === "discarded" ? "unsupported or duplicate findings skipped" : job.outcome?.startsWith("no_") ? "no supported source excerpts" : job.status}{job.model ? ` (${job.model})` : ""}</li>)}</ul>}
    </section>
  )
}
