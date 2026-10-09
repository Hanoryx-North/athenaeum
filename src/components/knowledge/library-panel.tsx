/**
 * The hosted Library, as far as this window can honestly see it.
 *
 * Publishing is not the desktop's job: the Library client lives in the CLI,
 * sync never runs in the background, and this window makes no network request
 * to a hosted service. What it *can* do is read the link the CLI recorded —
 * which repository this project is linked to, when it last agreed with it, and
 * whether anything is waiting for a person — and say plainly that reachability
 * is not something it checked.
 *
 * That is the offline/unreachable state this panel exists for: instead of a
 * green light nobody verified, it shows the link, the last sync the CLI
 * recorded, and the exact commands that publish or pull.
 */
import { useCallback, useEffect, useState } from "react"
import {
  AlertTriangle,
  Check,
  CloudOff,
  Copy,
  Link2Off,
  Loader2,
  RefreshCw,
  Unplug,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useEngineStore } from "@/stores/engine-store"
import { formatTimestamp } from "@/lib/engine-present"
import { cn } from "@/lib/utils"

function CommandLine({ command }: { command: string }) {
  const [copied, setCopied] = useState(false)

  const copy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(command)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch {
      // Clipboard access can be refused; the command is still selectable text.
      setCopied(false)
    }
  }, [command])

  return (
    <div className="mt-1 flex items-center gap-2">
      <code className="min-w-0 flex-1 select-all overflow-x-auto rounded bg-muted px-2 py-1 font-mono text-[11px]">
        {command}
      </code>
      <Button variant="ghost" size="sm" className="h-6 shrink-0 px-2 text-xs" onClick={() => void copy()}>
        {copied ? <Check className="h-3 w-3" aria-hidden /> : <Copy className="h-3 w-3" aria-hidden />}
        <span className="sr-only">Copy command</span>
        <span aria-hidden className="ml-1">
          {copied ? "Copied" : "Copy"}
        </span>
      </Button>
    </div>
  )
}

export function LibraryPanel() {
  const library = useEngineStore((s) => s.library)
  const libraryState = useEngineStore((s) => s.libraryState)
  const libraryError = useEngineStore((s) => s.libraryError)
  const refresh = useEngineStore((s) => s.refresh)
  const refreshing = useEngineStore((s) => s.refreshing)
  const loadLibrary = useEngineStore((s) => s.loadLibrary)
  const [checking, setChecking] = useState(false)

  useEffect(() => {
    if (!library && libraryState.kind === "loading") void loadLibrary()
  }, [library, libraryState.kind, loadLibrary])

  const check = useCallback(async () => {
    setChecking(true)
    try {
      await loadLibrary()
    } finally {
      setChecking(false)
    }
  }, [loadLibrary])

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
        <div>
          <h2 className="text-sm font-semibold">Library</h2>
          <p className="text-xs text-muted-foreground">
            The hosted store for accepted wiki text. This window never contacts it.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={() => void check()} disabled={checking}>
            {checking ? (
              <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden />
            ) : (
              <RefreshCw className="mr-1 h-3 w-3" aria-hidden />
            )}
            Recheck link
          </Button>
          <Button variant="outline" size="sm" className="h-7 text-xs" onClick={() => void refresh()} disabled={refreshing}>
            Refresh everything
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        <div className="mx-auto flex max-w-2xl flex-col gap-4">
          {libraryState.kind === "loading" && (
            <p className="flex items-center gap-2 text-sm text-muted-foreground" aria-live="polite">
              <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
              Reading the link the CLI recorded…
            </p>
          )}

          {libraryState.kind === "error" && (
            <section
              role="alert"
              className="rounded-md border border-destructive/40 bg-destructive/5 p-3 text-sm"
            >
              <div className="flex items-center gap-2 font-medium text-destructive">
                <AlertTriangle className="h-4 w-4" aria-hidden />
                The link state could not be read
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                {libraryError ?? "The sync state or athenaeum.json is unreadable."} Nothing was
                changed; publishing still runs from the CLI.
              </p>
              <Button variant="outline" size="sm" className="mt-2 h-7 text-xs" onClick={() => void check()}>
                Try again
              </Button>
            </section>
          )}

          {libraryState.kind === "unlinked" && (
            <section
              role="status"
              className="rounded-md border bg-muted/30 p-3 text-sm"
            >
              <div className="flex items-center gap-2 font-medium">
                <Link2Off className="h-4 w-4 text-muted-foreground" aria-hidden />
                Not linked to a Library
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                This project keeps everything locally: the Markdown vault is the source of truth,
                and there is nothing to be offline from. If you want this wiki published, the CLI
                links it — the desktop cannot, because the Library client lives there.
              </p>
              <CommandLine command="athenaeum library login --server https://your-library.example" />
              <CommandLine command="athenaeum sync link --create my-wiki" />
              <p className="mt-2 text-xs text-muted-foreground">
                Only accepted wiki Markdown is ever sent. Raw sources, code, indexes and local state
                stay here.
              </p>
            </section>
          )}

          {libraryState.kind === "conflicted" && (
            <section
              role="status"
              className="rounded-md border border-amber-500/40 bg-amber-500/5 p-3 text-sm"
            >
              <div className="flex items-center gap-2 font-medium text-amber-700 dark:text-amber-500">
                <AlertTriangle className="h-4 w-4" aria-hidden />
                Divergent edits are waiting for you
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                {libraryState.conflicts.length} path
                {libraryState.conflicts.length === 1 ? "" : "s"} changed on both sides. Nothing was
                overwritten: local work is intact until you choose.
              </p>
              <ul className="mt-2 list-inside list-disc break-all font-mono text-[11px] text-muted-foreground">
                {libraryState.conflicts.map((path) => (
                  <li key={path}>{path}</li>
                ))}
              </ul>
              <CommandLine
                command={`athenaeum sync resolve ${libraryState.conflicts[0]} --keep local|remote|both`}
              />
            </section>
          )}

          {libraryState.kind === "linked" && (
            <>
              <section role="status" className="rounded-md border p-3 text-sm">
                <div className="flex items-center gap-2 font-medium">
                  <CloudOff className="h-4 w-4 text-muted-foreground" aria-hidden />
                  Linked to {libraryState.remoteName ?? libraryState.remoteId}
                </div>
                <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs">
                  <dt className="text-muted-foreground">Server</dt>
                  <dd className="break-all font-mono">{libraryState.server}</dd>
                  <dt className="text-muted-foreground">Remote</dt>
                  <dd className="break-all font-mono">{libraryState.remoteId}</dd>
                  <dt className="text-muted-foreground">Last agreed</dt>
                  <dd>
                    {libraryState.lastSynced
                      ? `${libraryState.lastSynced} (${formatTimestamp(libraryState.lastSyncAt)})`
                      : "never"}
                  </dd>
                  <dt className="text-muted-foreground">Reachability</dt>
                  <dd>
                    <span className="rounded-full border border-border bg-muted/50 px-2 py-0.5 text-[11px] text-muted-foreground">
                      not checked from this window
                    </span>
                  </dd>
                </dl>
                <p className="mt-2 text-xs text-muted-foreground">
                  This window makes no network request to the Library, so it will not claim the
                  Library is up when it has not looked. Publishing, pulling and conflict resolution
                  run in the CLI, where the client and its credentials live.
                </p>
                <CommandLine command="athenaeum sync push" />
                <CommandLine command="athenaeum sync pull" />
              </section>

              {library?.repositoryId && (
                <section className="rounded-md border p-3 text-sm">
                  <h3 className="font-medium">Repository identity</h3>
                  <dl className="mt-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs">
                    <dt className="text-muted-foreground">Name</dt>
                    <dd>{library.repositoryName ?? "(unnamed)"}</dd>
                    <dt className="text-muted-foreground">ID</dt>
                    <dd className="break-all font-mono">{library.repositoryId}</dd>
                    <dt className="text-muted-foreground">Manifest</dt>
                    <dd>{library.manifestPresent ? "athenaeum.json present" : "no athenaeum.json"}</dd>
                  </dl>
                </section>
              )}
            </>
          )}

          <section
            className={cn(
              "rounded-md border border-dashed p-3 text-xs text-muted-foreground",
            )}
            aria-label="What never leaves this machine"
          >
            <div className="flex items-center gap-2 font-medium text-foreground">
              <Unplug className="h-3.5 w-3.5" aria-hidden />
              What sync never sends
            </div>
            <p className="mt-1">
              Raw sources, code repositories, the SQLite index, embeddings, agent sessions, logs and
              credentials. The cloud is a place to publish accepted text, not a place to think — and
              nothing here syncs in the background.
            </p>
          </section>
        </div>
      </div>
    </div>
  )
}
