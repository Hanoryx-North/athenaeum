/** Explicit Library operations. The CLI owns HTTP, credentials and sync policy. */
import { useEffect, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { SetupCard } from "@/components/setup/setup-card"
import { useEngineStore } from "@/stores/engine-store"
import { useSetupStore } from "@/stores/setup-store"
import { useWikiStore } from "@/stores/wiki-store"
import { useAppDialog } from "@/stores/app-dialog-store"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"
import type { CliJob } from "@/lib/cli-tools"

interface ConflictPreview { path: string; base: string | null; local: string | null; remote: string | null }

export function LibraryPanel() {
  const project = useWikiStore((s) => s.project)
  return project ? <LibraryWorkspace key={project.path} workspace={project.path} name={project.name} /> : null
}

function LibraryWorkspace({ workspace, name }: { workspace: string; name: string }) {
  const { library, libraryState, libraryError, loadLibrary, refresh } = useEngineStore()
  const { cli, busy, operate, syncStatus, syncPosition, error } = useSetupStore()
  const [server, setServer] = useState("https://wiki.hanoryx.com")
  const [wikiName, setWikiName] = useState(name.toLowerCase().replace(/[^a-z0-9-]/g, "-").slice(0, 60))
  const [isPublic, setPublic] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [preview, setPreview] = useState<ConflictPreview | null>(null)
  const dialog = useAppDialog()
  const linked = libraryState.kind === "linked" || libraryState.kind === "conflicted"
  const link = library
  const linkedServer = link?.server
  const working = busy !== null

  useEffect(() => { void loadLibrary() }, [loadLibrary])

  async function run(job: CliJob) {
    setNotice(null)
    const result = await operate(workspace, job)
    if (useWikiStore.getState().project?.path !== workspace) return null
    if (result?.exitCode === 0) {
      await loadLibrary()
      if (job.job !== "sync_conflict" && job.job !== "library_login") {
        await useSetupStore.getState().check(workspace)
        await refreshProjectFileTree(workspace, { bumpDataVersion: true })
        await refresh()
      }
      return result.output
    }
    await loadLibrary()
    return null
  }

  async function publish() {
    if (isPublic && !await dialog.confirm({ title: "Publish a public wiki?", message: "Everyone can read the explicitly shared, accepted pages you publish. Raw sources, proposals and local state stay on this computer.", confirmLabel: "Publish publicly" })) return
    const signedIn = await run({ job: "library_login", server })
    if (!signedIn) return
    const created = await run({ job: "sync_link", server, name: wikiName.trim(), public: isPublic })
    if (!created) return
    const pushed = await run({ job: "sync_push" })
    if (pushed) setNotice("Your wiki is published. Use Pull to receive updates and Push to share local accepted changes.")
    await syncStatus(workspace)
  }

  async function sync(job: "sync" | "sync_push" | "sync_pull") {
    const result = await run({ job })
    if (result) setNotice(job === "sync_pull" ? "Pull completed." : job === "sync_push" ? "Push completed." : "Sync completed.")
    await syncStatus(workspace)
  }

  async function inspect(path: string) {
    const out = await run({ job: "sync_conflict", path })
    if (out && typeof out === "object") {
      const p = out as ConflictPreview
      if (p.path === path && [p.base, p.local, p.remote].every((v) => v === null || typeof v === "string")) setPreview(p)
    }
  }

  async function resolve(keep: "local" | "remote" | "both") {
    if (!preview) return
    if (keep === "remote" && !await dialog.confirm({ title: "Use the Library version?", message: `Replace the local version of ${preview.path} with the reviewed Library version${preview.remote === null ? " (deleted)" : ""}?`, confirmLabel: "Use Library version" })) return
    const out = await run({ job: "sync_resolve", path: preview.path, keep })
    if (!out) return
    setPreview(null)
    setNotice(keep === "both" ? "Both versions are in the local page with conflict markers. Edit the page, remove the markers, then review it and choose Keep local. Push stays blocked until every conflict is resolved." : "Conflict resolved locally. Sync when all conflicts are resolved to share the result.")
    await syncStatus(workspace)
  }

  return <div className="h-full overflow-y-auto p-4"><div className="mx-auto max-w-3xl space-y-4">
    <header><h2 className="text-sm font-semibold">Library</h2><p className="mt-1 text-xs text-muted-foreground">Publish accepted Markdown and bring changes between computers. Every transfer runs when you ask.</p></header>
    <SetupCard />
    {libraryError && <p role="alert" className="text-sm text-destructive">{libraryError}</p>}
    {libraryState.kind === "loading" && <p role="status">Reading the local Library link?</p>}
    {libraryState.kind === "unlinked" && <section className="space-y-3 rounded-md border p-4" aria-label="Publish wiki">
      <h3 className="font-medium">Publish this wiki</h3>
      <p className="text-xs text-muted-foreground">Publish the accepted Markdown under wiki/ and the minimal repository identity. This explicitly shares those pages with the hosted wiki’s readers.</p>
      <label className="block text-xs">Library address<Input value={server} onChange={(e) => setServer(e.target.value)} disabled={working} /></label>
      <label className="block text-xs">Wiki name<Input value={wikiName} onChange={(e) => setWikiName(e.target.value)} disabled={working} /></label>
      <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={isPublic} disabled={working} onChange={(e) => setPublic(e.target.checked)} />Make this wiki public (private by default)</label>
      <Button disabled={working || !cli?.found || !wikiName.trim()} onClick={() => void publish()}>Sign in and publish</Button>
      <p className="text-xs text-muted-foreground">To download an existing hosted wiki, return to the project chooser and select Clone from Library.</p>
    </section>}
    {linked && <section className="space-y-3 rounded-md border p-4" aria-label="Library synchronization">
      <h3 className="font-medium">{link?.remoteName ?? "Linked wiki"}</h3>
      <p className="break-all text-xs text-muted-foreground">{link?.server} ? {link?.remoteId}</p>
      <p className="text-xs">Last agreed revision: <code className="break-all">{link?.lastSynced ?? "none yet"}</code></p>
      <p role="status" className="text-xs">Status: {syncPosition?.replace(/_/g, " ") ?? "not checked against the Library"}</p>
      <div className="flex flex-wrap gap-2">
        <Button variant="outline" disabled={working} onClick={() => void syncStatus(workspace)}>Check sync status</Button>
        <Button variant="outline" disabled={working} onClick={() => void sync("sync_pull")}>Pull</Button>
        <Button variant="outline" disabled={working || libraryState.kind === "conflicted"} onClick={() => void sync("sync_push")}>Push</Button>
        <Button disabled={working || libraryState.kind === "conflicted"} onClick={() => void sync("sync")}>Sync</Button>
      </div>
      <p className="text-xs text-muted-foreground">Pull merges non-overlapping edits and holds conflicts for review. Push sends explicitly shared accepted changes. Sync pulls, then pushes.</p>
      {linkedServer && <div className="flex flex-wrap gap-2">
        <Button variant="ghost" disabled={working} onClick={() => void run({ job: "library_login", server: linkedServer })}>Sign in again</Button>
        <Button variant="ghost" disabled={working} onClick={() => void (async () => {
          if (await dialog.confirm({ title: "Disconnect this wiki?", message: "Keep local pages and hosted history, and remove this computer?s link to the wiki.", confirmLabel: "Disconnect" })) await run({ job: "sync_unlink" })
        })()}>Disconnect wiki</Button>
        <Button variant="ghost" disabled={working} onClick={() => void run({ job: "library_logout", server: linkedServer })}>Sign out on this computer</Button>
        {link?.remoteName?.includes("/") && <Button variant="ghost" onClick={() => void openUrl(`${linkedServer}/${link.remoteName}/history`)}>View hosted history</Button>}
      </div>}
    </section>}
    {libraryState.kind === "conflicted" && <section className="space-y-2 rounded-md border border-amber-500 p-4" aria-label="Resolve conflicts">
      <h3 className="font-medium">Review conflicting pages</h3><p className="text-xs">Local edits are preserved. Compare each page before choosing a version. Push is blocked until all conflicts are resolved.</p>
      {libraryState.conflicts.map((path) => <div key={path} className="flex items-center justify-between gap-2"><code className="break-all text-xs">{path}</code><Button variant="outline" disabled={working} onClick={() => void inspect(path)}>Compare versions</Button></div>)}
      {preview && <div className="space-y-3"><h4 className="break-all text-sm font-medium">{preview.path}</h4>
        <div className="grid gap-2 md:grid-cols-2">{(["local", "remote"] as const).map((side) => <div key={side}><h5 className="text-xs font-medium">{side === "local" ? "Local version" : "Library version"}</h5><pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-2 text-xs">{preview[side] ?? "(deleted)"}</pre></div>)}</div>
        <details><summary className="cursor-pointer text-xs">Last shared version</summary><pre className="max-h-48 overflow-auto whitespace-pre-wrap text-xs">{preview.base ?? "(did not exist)"}</pre></details>
        <div className="flex flex-wrap gap-2"><Button disabled={working} onClick={() => void resolve("local")}>Keep local</Button><Button variant="outline" disabled={working} onClick={() => void resolve("remote")}>Use Library version</Button><Button variant="outline" disabled={working || preview.local === null || preview.remote === null} onClick={() => void resolve("both")}>Combine for manual editing</Button></div>
      </div>}
    </section>}
    {notice && <p role="status" className="text-sm">{notice}</p>}
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
    <p className="text-xs text-muted-foreground">Raw sources, source code, proposals, indexes, agent sessions, logs and credentials stay local. Offline browsing and editing do not need Library sign-in. Automatic sync is opt-in and runs only in a supported agent?s foreground turn.</p>
  </div></div>
}
