import { useEffect, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { BookOpen, CalendarDays, FileText, Plus } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { api } from "@/lib/knowledge-api"
import { openKnowledgeBase } from "@/lib/knowledge-index"
import { getFileStem, getRelativePath } from "@/lib/path-utils"
import { createNote, openDailyNote } from "@/lib/wiki-notes"
import { useAutomationStore } from "@/stores/automation-store"
import { useWikiStore } from "@/stores/wiki-store"

const dayFormat = new Intl.DateTimeFormat(undefined, { dateStyle: "full" })
const activityDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" })

interface SavedNote {
  path: string
  at: string
}

export function LogView({ workspace }: { workspace: string }) {
  const { t } = useTranslation()
  const [title, setTitle] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activity, setActivity] = useState<SavedNote[] | null>(null)
  const [activityError, setActivityError] = useState(false)
  const [reload, setReload] = useState(0)
  const index = useWikiStore((s) => s.projectPathIndex)
  const dataVersion = useWikiStore((s) => s.dataVersion)
  const open = useWikiStore((s) => s.openPathInPreview)
  const savingError = useAutomationStore((s) => s.workspace === workspace ? s.error : null)

  useEffect(() => {
    let cancelled = false
    async function load() {
      try {
        await openKnowledgeBase(workspace)
        if (cancelled || useWikiStore.getState().project?.path !== workspace) return
        const entries = await api.journal(40, "node_accepted", workspace)
        if (cancelled) return
        const notes = new Map<string, SavedNote>()
        for (const entry of entries) {
          const detail = entry.detail as { path?: unknown } | null
          const path = detail?.path
          if (typeof path !== "string" || !path.startsWith("wiki/") || path.includes("\\") || path.split("/").includes("..")) continue
          if (!notes.has(path)) notes.set(path, { path: `${workspace}/${path}`, at: entry.at })
          if (notes.size === 12) break
        }
        setActivity([...notes.values()])
        setActivityError(false)
      } catch {
        if (!cancelled) setActivityError(true)
      }
    }
    void load()
    return () => { cancelled = true }
  }, [workspace, dataVersion, reload])

  async function capture(action: () => Promise<string>) {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await action()
      setTitle(null)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  const dailyNotes = useMemo(() => [...index.byPath.keys()]
    .filter(path => /^wiki\/daily\/\d{4}-\d{2}-\d{2}\.md$/.test(getRelativePath(path, workspace)))
    .sort((a, b) => b.localeCompare(a)).slice(0, 10), [index, workspace])
  const history = `${workspace}/wiki/log.md`
  const recent = activity?.filter(note => index.byPath.has(note.path))

  return (
    <main className="h-full overflow-auto">
      <div className="mx-auto max-w-3xl space-y-8 px-5 py-8 sm:px-10 sm:py-12">
        <header className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">{t("nav.log")}</h1>
            <p className="mt-1 text-sm text-muted-foreground">{dayFormat.format(new Date())}</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" onClick={() => setTitle("")} disabled={busy || title !== null}>
              <Plus className="h-4 w-4" /> New note
            </Button>
            <Button onClick={() => void capture(openDailyNote)} disabled={busy}>
              <CalendarDays className="h-4 w-4" /> Today’s note
            </Button>
          </div>
        </header>

        {title !== null && <form className="space-y-2 border-y py-4" onSubmit={event => {
          event.preventDefault()
          if (title.trim()) void capture(() => createNote(title))
        }}>
          <label htmlFor="new-note-title" className="text-sm font-medium">Note title</label>
          <div className="flex flex-wrap items-center gap-2">
            <Input id="new-note-title" className="min-w-36 flex-1" autoFocus value={title} maxLength={200}
              onChange={event => setTitle(event.target.value)} disabled={busy} />
            <Button type="submit" disabled={busy || !title.trim()}>Create note</Button>
            <Button type="button" variant="ghost" disabled={busy} onClick={() => { setTitle(null); setError(null) }}>Cancel</Button>
          </div>
        </form>}
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
        {savingError && <p role="alert" className="whitespace-pre-line text-sm text-destructive">Could not save wiki updates. Athenaeum will retry. {savingError}</p>}

        {dailyNotes.length > 0 && <section aria-labelledby="daily-notes-heading">
          <h2 id="daily-notes-heading" className="mb-2 text-sm font-semibold">Daily notes</h2>
          <ul className="divide-y border-y">
            {dailyNotes.map(path => <LogRow key={path} title={getFileStem(path)} onOpen={() => open(path)} />)}
          </ul>
        </section>}

        <section aria-labelledby="recent-saves-heading">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <h2 id="recent-saves-heading" className="text-sm font-semibold">Recent saves</h2>
            {index.byPath.has(history) && <Button variant="ghost" size="sm" onClick={() => open(history)}>
              <BookOpen className="h-4 w-4" /> Wiki history
            </Button>}
          </div>
          {activityError ? <div className="flex flex-wrap items-center gap-2 border-t pt-2">
            <p role="alert" className="text-sm text-destructive">Could not load recent saves.</p>
            <Button size="sm" variant="ghost" onClick={() => setReload(reload + 1)}>Retry</Button>
          </div> : recent?.length ? <ul className="divide-y border-y">
            {recent.map(({ path, at }) => <LogRow key={path} title={getFileStem(path)}
              at={at} onOpen={() => open(path)} />)}
          </ul> : <p role="status" className="border-t py-3 text-sm text-muted-foreground">
            {recent === undefined ? "Loading…" : "Saved agent notes will appear here."}
          </p>}
        </section>
      </div>
    </main>
  )
}

function LogRow({ title, at, onOpen }: { title: string; at?: string; onOpen: () => void }) {
  const date = at ? new Date(at) : null
  return <li>
    <Button variant="ghost" className="h-auto min-h-10 w-full justify-start gap-3 px-2 text-left" onClick={onOpen}>
      <FileText className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 truncate">{title}</span>
      {date && Number.isFinite(date.getTime()) && <time dateTime={at} className="shrink-0 text-xs text-muted-foreground">
        {activityDate.format(date)}
      </time>}
    </Button>
  </li>
}
