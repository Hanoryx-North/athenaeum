/** Explicit Library operations. The CLI owns HTTP, credentials and sync policy. */
import { useEffect, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { LibraryAccount } from "@/components/setup/library-account"
import { useEngineStore } from "@/stores/engine-store"
import { useSetupStore } from "@/stores/setup-store"
import { useWikiStore } from "@/stores/wiki-store"
import { useAppDialog } from "@/stores/app-dialog-store"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"
import type { CliJob } from "@/lib/cli-tools"
import { useAutomationStore } from "@/stores/automation-store"
import { LIBRARY, useLibraryAccountStore } from "@/stores/library-account-store"

interface ConflictPreview {
  path: string
  base: string | null
  local: string | null
  remote: string | null
}

export function LibraryPanel() {
  const project = useWikiStore((s) => s.project)
  return project ? (
    <LibraryWorkspace key={project.path} workspace={project.path} name={project.name} />
  ) : null
}

function LibraryWorkspace({ workspace, name }: { workspace: string; name: string }) {
  const { library, libraryState, libraryError, loadLibrary, refresh } = useEngineStore()
  const { cli, busy, operate, syncStatus, syncPosition, error } = useSetupStore()
  const [server, setServer] = useState("https://wiki.hanoryx.com")
  const [wikiName, setWikiName] = useState(
    name
      .toLowerCase()
      .replace(/[^a-z0-9-]/g, "-")
      .slice(0, 60),
  )
  const [isPublic, setPublic] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [preview, setPreview] = useState<ConflictPreview | null>(null)
  const [publishStep, setPublishStep] = useState<"details" | "account" | "review">("details")
  const [reauthenticating, setReauthenticating] = useState(false)
  const dialog = useAppDialog()
  const linked = libraryState.kind === "linked" || libraryState.kind === "conflicted"
  const link = library
  const linkedServer = link?.server
  const working = busy !== null
  const automation = useAutomationStore()
  const username = useLibraryAccountStore((state) => state.username)
  const remotePath = link?.remoteName?.includes("/") ? link.remoteName
    : username && linkedServer === LIBRARY && link?.remoteName ? `${username}/${link.remoteName}` : null

  useEffect(() => {
    void loadLibrary()
  }, [loadLibrary])

  async function run(job: CliJob) {
    setNotice(null)
    const result = await operate(workspace, job)
    if (useWikiStore.getState().project?.path !== workspace) return null
    if (result?.exitCode === 0) {
      await loadLibrary()
      if (!["sync_conflict", "library_login", "library_status"].includes(job.job)) {
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
    if (
      isPublic &&
      !(await dialog.confirm({
        title: "Publish a public wiki?",
        message:
          "Everyone can read the explicitly shared, accepted pages you publish. Raw sources, proposals and local state stay on this computer.",
        confirmLabel: "Publish publicly",
      }))
    )
      return
    const created = await run({ job: "sync_link", server, name: wikiName.trim(), public: isPublic })
    if (!created) return
    const pushed = await run({ job: "sync_push" })
    if (pushed)
      setNotice(
        "Your wiki is published. Use Pull to receive updates and Push to share local accepted changes.",
      )
    await syncStatus(workspace)
  }

  async function nextPublish() {
    const status = await run({ job: "library_status", server })
    if (
      Array.isArray(status) &&
      status.some((row) => typeof row?.username === "string" && !row.detail)
    )
      setPublishStep("review")
    else setPublishStep("account")
  }

  async function sync(job: "sync" | "sync_push" | "sync_pull") {
    const result = await run({ job })
    if (result)
      setNotice(
        job === "sync_pull"
          ? "Pull completed."
          : job === "sync_push"
            ? "Push completed."
            : "Sync completed.",
      )
    await syncStatus(workspace)
  }

  async function inspect(path: string) {
    const out = await run({ job: "sync_conflict", path })
    if (out && typeof out === "object") {
      const p = out as ConflictPreview
      if (
        p.path === path &&
        [p.base, p.local, p.remote].every((v) => v === null || typeof v === "string")
      )
        setPreview(p)
    }
  }

  async function resolve(keep: "local" | "remote" | "both") {
    if (!preview) return
    if (
      keep === "remote" &&
      !(await dialog.confirm({
        title: "Use the Library version?",
        message: `Replace the local version of ${preview.path} with the reviewed Library version${preview.remote === null ? " (deleted)" : ""}?`,
        confirmLabel: "Use Library version",
      }))
    )
      return
    const out = await run({ job: "sync_resolve", path: preview.path, keep })
    if (!out) return
    setPreview(null)
    setNotice(
      keep === "both"
        ? "Both versions are in the local page with conflict markers. Edit the page, remove the markers, then review it and choose Keep local. Push stays blocked until every conflict is resolved."
        : "Conflict resolved locally. Sync when all conflicts are resolved to share the result.",
    )
    await syncStatus(workspace)
  }

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mx-auto max-w-3xl space-y-4">
        <header>
          <h2 className="text-sm font-semibold">Library</h2>
          <p className="mt-1 text-xs text-muted-foreground">
            Keep your saved pages available across computers through your Library account.
          </p>
        </header>
        {libraryError && (
          <p role="alert" className="text-sm text-destructive">
            {libraryError}
          </p>
        )}
        {libraryState.kind === "loading" && (
          <p role="status">Loading your wiki’s Library connection…</p>
        )}
        {libraryState.kind === "unlinked" && (
          <section className="space-y-3 rounded-md border p-4" aria-label="Publish wiki">
            {publishStep === "account" ? (
              <>
                <LibraryAccount server={server} onContinue={() => setPublishStep("review")} />
                <Button
                  variant="ghost"
                  disabled={working}
                  onClick={() => setPublishStep("details")}
                >
                  Back
                </Button>
              </>
            ) : publishStep === "details" ? (
              <>
                <h3 className="text-lg font-medium">Publish this wiki</h3>
                <p className="text-sm text-muted-foreground">
                  Your account is connected separately from each wiki. Connect this wiki once to start syncing its saved pages.
                </p>
                <label className="block text-xs">
                  Wiki name
                  <Input
                    value={wikiName}
                    onChange={(e) => setWikiName(e.target.value)}
                    disabled={working}
                  />
                </label>
                <label className="flex items-center gap-2 text-xs">
                  <input
                    type="checkbox"
                    checked={isPublic}
                    disabled={working}
                    onChange={(e) => setPublic(e.target.checked)}
                  />
                  Make this wiki public (private by default)
                </label>
                <details className="text-xs text-muted-foreground">
                  <summary className="cursor-pointer">Advanced</summary>
                  <label className="mt-3 block">
                    Library address
                    <Input
                      value={server}
                      onChange={(e) => setServer(e.target.value)}
                      disabled={working}
                    />
                  </label>
                </details>
                <div className="flex justify-end border-t pt-4">
                  <Button
                    disabled={working || !cli?.found || !wikiName.trim()}
                    onClick={() => void nextPublish()}
                  >
                    Next
                  </Button>
                </div>
              </>
            ) : (
              <>
                <h3 className="text-lg font-medium">Ready to publish?</h3>
                <p className="text-sm text-muted-foreground">
                  Create <strong className="text-foreground">{wikiName}</strong> as a{" "}
                  {isPublic ? "public" : "private"} wiki in your Library account and upload its
                  accepted pages.
                </p>
                <p className="text-xs text-muted-foreground">
                  You can sync changes from this page after publishing.
                </p>
                <div className="flex justify-between border-t pt-4">
                  <Button
                    variant="ghost"
                    disabled={working}
                    onClick={() => setPublishStep("details")}
                  >
                    Back
                  </Button>
                  <Button disabled={working} onClick={() => void publish()}>
                    {working ? "Publishing…" : "Publish wiki"}
                  </Button>
                </div>
              </>
            )}
          </section>
        )}
        {linked && (
          <section className="space-y-3 rounded-md border p-4" aria-label="Library synchronization">
            <h3 className="font-medium">{link?.remoteName ?? "Linked wiki"}</h3>
            <label className="flex items-center gap-2 text-sm">
              <input type="checkbox" checked={automation.workspace === workspace && automation.settings.autoSync}
                disabled={automation.busy}
                onChange={(event) => void automation.run(workspace, {
                  settings: { ...automation.settings, autoSync: event.target.checked },
                })} />
              Back up automatically while Athenaeum is running
            </label>
            <p className="text-xs text-muted-foreground">Includes wiki pages and root Markdown files such as README, purpose and schema. Checks every minute, including when minimized, and retries after reconnecting. Conflicting edits wait for your review.</p>
            {automation.error && <p role="alert" className="text-xs text-destructive">{automation.error}</p>}
            <p className="break-all text-xs text-muted-foreground">{link?.server}</p>
            <p className="text-xs">
              Last agreed revision:{" "}
              <code className="break-all">{link?.lastSynced ?? "none yet"}</code>
            </p>
            <p role="status" className="text-xs">
              Status: {syncPosition?.replace(/_/g, " ") ?? "not checked against the Library"}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                disabled={working}
                onClick={() => void syncStatus(workspace)}
              >
                Check sync status
              </Button>
              <Button variant="outline" disabled={working} onClick={() => void sync("sync_pull")}>
                Pull
              </Button>
              <Button
                variant="outline"
                disabled={working || libraryState.kind === "conflicted"}
                onClick={() => void sync("sync_push")}
              >
                Push
              </Button>
              <Button
                disabled={working || libraryState.kind === "conflicted"}
                onClick={() => void sync("sync")}
              >
                Sync
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              Pull merges non-overlapping edits and holds conflicts for review. Push sends
              wiki pages and root Markdown files. Sync pulls, then pushes.
            </p>
            {linkedServer && (
              <div className="flex flex-wrap gap-2">
                <Button
                  variant="ghost"
                  disabled={working}
                  onClick={() => setReauthenticating(true)}
                >
                  Account
                </Button>
                <Button
                  variant="ghost"
                  disabled={working}
                  onClick={() =>
                    void (async () => {
                      if (
                        await dialog.confirm({
                          title: "Disconnect this wiki?",
                          message:
                            "Keep local pages and hosted history, and remove this computer’s link to the wiki.",
                          confirmLabel: "Disconnect",
                        })
                      )
                        await run({ job: "sync_unlink" })
                    })()
                  }
                >
                  Disconnect wiki
                </Button>
                {remotePath && (
                  <Button
                    variant="ghost"
                    onClick={() => void openUrl(`${linkedServer}/${remotePath}/commits`)}
                  >
                    View hosted history
                  </Button>
                )}
              </div>
            )}
          </section>
        )}
        {reauthenticating && linkedServer && (
          <section className="rounded-xl border p-5">
            <LibraryAccount server={linkedServer} onContinue={() => setReauthenticating(false)} />
          </section>
        )}
        {libraryState.kind === "conflicted" && (
          <section
            className="space-y-2 rounded-md border border-amber-500 p-4"
            aria-label="Resolve conflicts"
          >
            <h3 className="font-medium">Review conflicting pages</h3>
            <p className="text-xs">
              Local edits are preserved. Compare each page before choosing a version. Push is
              blocked until all conflicts are resolved.
            </p>
            {libraryState.conflicts.map((path) => (
              <div key={path} className="flex items-center justify-between gap-2">
                <code className="break-all text-xs">{path}</code>
                <Button variant="outline" disabled={working} onClick={() => void inspect(path)}>
                  Compare versions
                </Button>
              </div>
            ))}
            {preview && (
              <div className="space-y-3">
                <h4 className="break-all text-sm font-medium">{preview.path}</h4>
                <div className="grid gap-2 md:grid-cols-2">
                  {(["local", "remote"] as const).map((side) => (
                    <div key={side}>
                      <h5 className="text-xs font-medium">
                        {side === "local" ? "Local version" : "Library version"}
                      </h5>
                      <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-muted p-2 text-xs">
                        {preview[side] ?? "(deleted)"}
                      </pre>
                    </div>
                  ))}
                </div>
                <details>
                  <summary className="cursor-pointer text-xs">Last shared version</summary>
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap text-xs">
                    {preview.base ?? "(did not exist)"}
                  </pre>
                </details>
                <div className="flex flex-wrap gap-2">
                  <Button disabled={working} onClick={() => void resolve("local")}>
                    Keep local
                  </Button>
                  <Button
                    variant="outline"
                    disabled={working}
                    onClick={() => void resolve("remote")}
                  >
                    Use Library version
                  </Button>
                  <Button
                    variant="outline"
                    disabled={working || preview.local === null || preview.remote === null}
                    onClick={() => void resolve("both")}
                  >
                    Combine for manual editing
                  </Button>
                </div>
              </div>
            )}
          </section>
        )}
        {notice && (
          <p role="status" className="text-sm">
            {notice}
          </p>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
        <p className="text-xs text-muted-foreground">
          Your source files and coding sessions stay on this computer. Account and agent connections
          are in Connectors.
        </p>
      </div>
    </div>
  )
}
