import { useState } from "react"
import { invoke } from "@tauri-apps/api/core"
import { useTranslation } from "react-i18next"
import { Button } from "@/components/ui/button"
import { Label } from "@/components/ui/label"

export function KnowledgeIntake({ workspace }: { workspace: string }) {
  const { t } = useTranslation()
  const [text, setText] = useState("")
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState("")
  const [error, setError] = useState("")
  const oversized = new TextEncoder().encode(text).length > 64 * 1024
  const canSubmit = !busy && !!text.trim() && !oversized

  const submit = async () => {
    if (!canSubmit) return
    setBusy(true); setError(""); setMessage("")
    try {
      const result = await invoke<{ status: "queued" | "already_queued" }>("background_memory", {
        projectPath: workspace, action: "ingest", document: { text, wikiRoot: workspace },
      })
      setText("")
      setMessage(t(result.status === "already_queued" ? "sources.intake.duplicate" : "sources.intake.queued"))
    } catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { setBusy(false) }
  }
  return (
    <form className="space-y-3 border-b px-4 py-3" onSubmit={(event) => { event.preventDefault(); void submit() }}>
      <div className="space-y-2">
        <Label htmlFor="intake-text">{t("sources.intake.label")}</Label>
        <textarea id="intake-text" className="min-h-32 w-full resize-y rounded-md border bg-background p-2 text-sm focus-visible:outline-2 focus-visible:outline-ring" value={text}
          onChange={(event) => { setText(event.target.value); setMessage(""); setError("") }} disabled={busy} />
      </div>
      {oversized && <p role="alert" className="text-sm text-destructive">{t("sources.intake.tooLong")}</p>}
      <Button type="submit" size="sm" disabled={!canSubmit}>{t(busy ? "sources.intake.submitting" : "sources.intake.submit")}</Button>
      {message && <p role="status" className="text-sm">{message}</p>}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    </form>
  )
}
