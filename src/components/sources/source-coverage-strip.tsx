/**
 * What the engine did with the sources sitting in this folder.
 *
 * The sources tree below shows files; it cannot say which of them the engine
 * has read, because the file listing is not the index. This strip answers that
 * question for the raw sources specifically — how many the engine scanned, how
 * many it has never looked at, how many changed since — with the counts taken
 * from the engine's own `sources` records, never guessed from the tree.
 *
 * A source the engine never scanned is called unscanned here, in the folder a
 * person imports things into. "Not found in the graph" and "never looked at"
 * are different facts.
 */
import { useCallback, useState } from "react"
import { AlertTriangle, CheckCircle2, Database, Loader2, Wrench } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useKnowledgeViewStore } from "@/stores/knowledge-view-store"
import { sourceArea, sourceCoverage } from "@/lib/engine-present"
import { cn } from "@/lib/utils"

export function SourceCoverageStrip() {
  const setActiveView = useWikiStore((s) => s.setActiveView)
  const status = useEngineStore((s) => s.status)
  const sources = useEngineStore((s) => s.sources)
  const state = useEngineStore((s) => s.state)
  const refreshing = useEngineStore((s) => s.refreshing)
  const refresh = useEngineStore((s) => s.refresh)
  const rebuildIndex = useEngineStore((s) => s.rebuildIndex)
  const setTab = useKnowledgeViewStore((s) => s.setTab)
  const [rebuilding, setRebuilding] = useState(false)

  // One record per scan: a raw source the vault pass read and the repository
  // pass skipped is two records, and only one of them is covered.
  const raw = sources.filter((source) => sourceArea(source.path) === "raw")
  const coverage = sourceCoverage(raw)

  const openCoverage = useCallback(() => {
    setTab("sources")
    setActiveView("knowledge")
  }, [setActiveView, setTab])

  const rebuild = useCallback(async () => {
    setRebuilding(true)
    try {
      await rebuildIndex()
    } finally {
      setRebuilding(false)
    }
  }, [rebuildIndex])

  // The knowledge view owns the loud product states; here a quiet line is
  // enough, and it must never imply the folder was read when it was not.
  if (
    state.kind === "protocol-mismatch" ||
    state.kind === "unreachable" ||
    state.kind === "engine-error" ||
    state.kind === "absent-vault"
  ) {
    return null
  }

  const loading = refreshing && sources.length === 0 && status === null

  return (
    <div
      className={cn(
        "flex flex-wrap items-center gap-x-3 gap-y-1 border-b px-4 py-2 text-xs",
        coverage.records > 0 && coverage.complete ? "bg-emerald-500/5" : "bg-muted/30",
      )}
      role="status"
      aria-label="Engine scan coverage of raw sources"
    >
      {loading ? (
        <span className="flex items-center gap-1.5 text-muted-foreground">
          <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
          Asking the engine what it scanned…
        </span>
      ) : (
        <>
          {coverage.complete ? (
            <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-600 dark:text-emerald-500" aria-hidden />
          ) : (
            <AlertTriangle className="h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-500" aria-hidden />
          )}
          <span className="font-medium">Engine scan</span>
          <span className="text-muted-foreground">
            {coverage.records === 0
              ? status?.indexedAt
                ? "no raw source is recorded in the index"
                : "not indexed yet"
              : `${coverage.scanned} of ${coverage.records} raw source scan${coverage.records === 1 ? "" : "s"} read`}
            {coverage.unscanned > 0 && (
              <span
                className="text-amber-700 dark:text-amber-500"
                title={`Never read: ${coverage.unscannedPaths.slice(0, 5).join(", ")}${coverage.unscannedPaths.length > 5 ? `, +${coverage.unscannedPaths.length - 5} more` : ""}`}
              >
                {" "}
                · {coverage.unscanned} never read
              </span>
            )}
            {coverage.stale > 0 && (
              <span
                className="text-amber-700 dark:text-amber-500"
                title={`Changed since the scan: ${coverage.stalePaths.slice(0, 5).join(", ")}${coverage.stalePaths.length > 5 ? `, +${coverage.stalePaths.length - 5} more` : ""}`}
              >
                {" "}
                · {coverage.stale} changed since
              </span>
            )}
            {coverage.records > 0 && ` · ${coverage.anchors} anchor${coverage.anchors === 1 ? "" : "s"} · ${coverage.nodes} node${coverage.nodes === 1 ? "" : "s"}`}
          </span>
          <span className="ml-auto flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => void refresh()}
              disabled={refreshing}
            >
              Recheck
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-6 px-2 text-xs"
              onClick={() => void rebuild()}
              disabled={rebuilding}
            >
              <Wrench className="mr-1 h-3 w-3" aria-hidden />
              {rebuilding ? "Rebuilding…" : "Rebuild index"}
            </Button>
            <Button variant="ghost" size="sm" className="h-6 px-2 text-xs" onClick={openCoverage}>
              <Database className="mr-1 h-3 w-3" aria-hidden />
              Scan coverage
            </Button>
          </span>
        </>
      )}
    </div>
  )
}
