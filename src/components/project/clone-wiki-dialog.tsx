import { useState } from "react"
import { open } from "@tauri-apps/plugin-dialog"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { cliStatus, installTools, onCliProgress, runCli, cancelCli, type JobResult } from "@/lib/cli-tools"
import { openProject } from "@/commands/fs"
import { normalizePath } from "@/lib/path-utils"
import type { WikiProject } from "@/types/wiki"

export function CloneWikiDialog({ open: visible, onOpenChange, onCloned }: {
  open: boolean; onOpenChange: (open: boolean) => void; onCloned: (project: WikiProject) => void
}) {
  const [server, setServer] = useState("https://wiki.hanoryx.com")
  const [remote, setRemote] = useState("")
  const [parent, setParent] = useState("")
  const [folder, setFolder] = useState("")
  const [signIn, setSignIn] = useState(true)
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [device, setDevice] = useState<{ url: string; code: string } | null>(null)

  async function clone() {
    setError(null)
    if (!parent || !remote.trim() || !/^[A-Za-z0-9][A-Za-z0-9._ -]{0,79}$/.test(folder) || /[. ]$/.test(folder)) {
      setError("Choose a parent folder, enter the hosted owner/wiki or repository ID, and give the new folder a name.")
      return
    }
    setBusy(true)
    let stop: (() => void) | undefined
    const requireSuccess = (result: JobResult) => {
      if (result.exitCode !== 0) {
        const out = result.output as { error?: { message?: string }; message?: string } | null
        throw new Error(out?.error?.message ?? out?.message ?? result.stderr[result.stderr.length - 1] ?? "The operation stopped. Retry after checking the Library connection.")
      }
    }
    try {
      stop = await onCliProgress((p) => {
        setProgress(p.line)
        if (p.event?.event === "device_code" && typeof p.event.url === "string" && typeof p.event.code === "string") {
          setDevice({ url: p.event.url, code: p.event.code })
        }
      })
      if (!(await cliStatus()).found) {
        setProgress("Installing the packaged local tools…")
        requireSuccess(await installTools(false))
      }
      if (signIn) {
        setProgress("Waiting for Library sign-in…")
        requireSuccess(await runCli(parent, { job: "library_login", server }))
        setDevice(null)
      }
      const destination = `${normalizePath(parent).replace(/\/$/, "")}/${folder}`
      setProgress("Downloading accepted pages and rebuilding the local index…")
      requireSuccess(await runCli(parent, { job: "sync_clone", server, remote: remote.trim(), destination }))
      const project = await openProject(destination)
      onOpenChange(false)
      onCloned(project)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      stop?.()
      setBusy(false)
      setDevice(null)
    }
  }

  return <Dialog open={visible} onOpenChange={(next) => { if (!busy) onOpenChange(next) }}>
    <DialogContent className="sm:max-w-lg"><DialogHeader><DialogTitle>Clone from Library</DialogTitle></DialogHeader>
      <p className="text-xs text-muted-foreground">Download a hosted wiki into a new local folder. Its accepted Markdown is available offline and its index is rebuilt locally.</p>
      <fieldset disabled={busy} className="space-y-3">
        <label className="block text-xs">Library address<Input value={server} onChange={(e) => setServer(e.target.value)} /></label>
        <label className="block text-xs">Hosted wiki (owner/wiki or repository ID)<Input value={remote} onChange={(e) => setRemote(e.target.value)} /></label>
        <label className="block text-xs">New folder name<Input value={folder} onChange={(e) => setFolder(e.target.value)} /></label>
        <div className="space-y-1"><span className="text-xs">Save inside</span><Button variant="outline" className="w-full justify-start overflow-hidden" onClick={() => void (async () => { const selected = await open({ directory: true, multiple: false, title: "Choose where to clone this wiki" }); if (selected) setParent(selected) })()}>{parent || "Choose a parent folder…"}</Button></div>
        <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={signIn} onChange={(e) => setSignIn(e.target.checked)} />Sign in through my browser (required for private wikis)</label>
      </fieldset>
      {device && <div role="status" className="space-y-2 rounded border p-3"><p className="text-xs">Confirm this code in Library: <strong>{device.code}</strong></p><Button variant="outline" onClick={() => void openUrl(device.url)}>Open Library sign-in</Button></div>}
      {error && <p role="alert" className="text-xs text-destructive">{error}</p>}
      {busy && <p role="status" className="break-all text-xs text-muted-foreground">{progress}</p>}
      <div className="flex justify-end gap-2">{busy ? <Button variant="outline" onClick={() => void cancelCli()}>Cancel clone</Button> : <Button variant="outline" onClick={() => onOpenChange(false)}>Cancel</Button>}<Button disabled={busy || !parent || !folder || !remote.trim()} onClick={() => void clone()}>{busy ? "Cloning…" : "Clone wiki"}</Button></div>
    </DialogContent>
  </Dialog>
}
