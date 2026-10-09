/**
 * What the engine actually read.
 *
 * The list is the engine's own record of the last index pass: every file it
 * tracked, project-relative, with its hash, how many nodes and anchors rest on
 * it, and when it was indexed. Coverage is stated plainly — how many files
 * were scanned, how many were not, how many changed since — because "we did
 * not look" and "we looked and found nothing" are different facts, and only
 * one of them is safe to act on.
 *
 * A file the engine has never scanned is shown as unscanned. It is never
 * counted as covered, and it is never hidden.
 */
import { useCallback, useMemo, useState } from "react"
import {
  AlertTriangle,
  CheckCircle2,
  FileSearch,
  Loader2,
  RefreshCw,
  Search,
  Wrench,
  XCircle,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import type { EngineSource } from "@/lib/engine-client"
import {
  SOURCE_AREA_LABELS,
  formatAge,
  formatBytes,
  orderSourceRecords,
  shortHash,
  sourceArea,
  sourceCoverage,
  sourceScopeLabel,
  type SourceArea,
} from "@/lib/engine-present"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"

type AreaFilter = "all" | SourceArea
type CoverageFilter = "all" | "unscanned" | "stale"

export function KnowledgeSources() {
  const project = useWikiStore((s) => s.project)
  const openPage = useWikiStore((s) => s.openPathInPreview)
  const sources = useEngineStore((s) => s.sources)
  const sourcesError = useEngineStore((s) => s.sourcesError)
  const status = useEngineStore((s) => s.status)
  const refreshing = useEngineStore((s) => s.refreshing)
  const refresh = useEngineStore((s) => s.refresh)
  const rebuildIndex = useEngineStore((s) => s.rebuildIndex)

  const [area, setArea] = useState<AreaFilter>("raw")
  const [coverage, setCoverage] = useState<CoverageFilter>("all")
  const [query, setQuery] = useState("")
  const [rebuilding, setRebuilding] = useState(false)

  const workspace = project ? normalizePath(project.path).replace(/\/+$/, "") : null
  // One row per (scan, path): the engine records a file once per scan that
  // read it, and a scan that skipped it stays visible as skipped.
  const ordered = useMemo(() => orderSourceRecords(sources), [sources])
  const summary = useMemo(() => sourceCoverage(sources), [sources])

  const areaCounts = useMemo(() => {
    const counts: Record<AreaFilter, number> = { all: ordered.length, raw: 0, vault: 0, repository: 0 }
    for (const source of ordered) counts[sourceArea(source.path)] += 1
    return counts
  }, [ordered])

  const visible = useMemo(() => {
    const needle = query.trim().toLowerCase()
    return ordered.filter((source) => {
      if (area !== "all" && sourceArea(source.path) !== area) return false
      if (coverage === "unscanned" && source.scanned) return false
      if (coverage === "stale" && !source.stale) return false
      if (needle && !source.path.toLowerCase().includes(needle)) return false
      return true
    })
  }, [ordered, area, coverage, query])

  const handleRebuild = useCallback(async () => {
    setRebuilding(true)
    try {
      await rebuildIndex()
    } finally {
      setRebuilding(false)
    }
  }, [rebuildIndex])

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="text-sm font-semibold">Scan coverage</h2>
          <span className="text-xs text-muted-foreground">
            What the last index pass read, as the engine recorded it
          </span>
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs"
            onClick={() => void refresh()}
            disabled={refreshing}
          >
            <RefreshCw className={cn("mr-1 h-3 w-3", refreshing && "animate-spin")} aria-hidden />
            Re-read
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={() => void handleRebuild()}
            disabled={rebuilding}
          >
            <Wrench className="mr-1 h-3 w-3" aria-hidden />
            {rebuilding ? "Rebuilding…" : "Rebuild index"}
          </Button>
        </div>
      </div>

      <dl className="grid grid-cols-2 gap-px border-b bg-border text-xs sm:grid-cols-3 lg:grid-cols-6">
        <StatCell label="Scan records" value={summary.records} />
        <StatCell label="Scanned" value={summary.scanned} tone="good" />
        <StatCell label="Never scanned" value={summary.unscanned} tone={summary.unscanned > 0 ? "bad" : "muted"} />
        <StatCell label="Changed since" value={summary.stale} tone={summary.stale > 0 ? "warn" : "muted"} />
        <StatCell label="Anchors" value={summary.anchors} />
        <StatCell label="Nodes" value={summary.nodes} />
      </dl>

      <div
        className={cn(
          "flex items-start gap-2 border-b px-4 py-2 text-xs",
          summary.complete ? "bg-emerald-500/5 text-emerald-700 dark:text-emerald-500" : "bg-amber-500/5",
        )}
        role="status"
      >
        {summary.complete ? (
          <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
        ) : (
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-600 dark:text-amber-500" aria-hidden />
        )}
        <p className="min-w-0 flex-1 text-muted-foreground">
          {summary.records === 0 ? (
            status?.indexedAt
              ? "The engine indexed this project and reported no files at all, which is worth a look."
              : "No index pass has recorded any file yet, so nothing here is covered."
          ) : summary.complete ? (
            `Every scan record was read and nothing has changed since${status?.indexedAt ? ` (last pass ${formatAge(status.indexedAt)})` : ""}.`
          ) : (
            <>
              {summary.unscanned > 0 && (
                <>
                  {summary.unscanned} scan record{summary.unscanned === 1 ? "" : "s"} never read
                </>
              )}
              {summary.unscanned > 0 && summary.stale > 0 && " · "}
              {summary.stale > 0 && (
                <>
                  {summary.stale} changed since the scan
                </>
              )}
              {" — coverage is partial, so numbers elsewhere are a floor."}
            </>
          )}
        </p>
      </div>

      {sourcesError && (
        <p role="alert" className="border-b border-destructive/40 bg-destructive/5 px-4 py-2 text-xs text-destructive">
          {sourcesError}
        </p>
      )}

      <div className="flex flex-wrap items-center gap-2 border-b bg-muted/20 px-4 py-2 text-xs">
        {(["all", "raw", "vault", "repository"] as AreaFilter[]).map((option) => (
          <button
            key={option}
            aria-pressed={area === option}
            onClick={() => setArea(option)}
            className={cn(
              "rounded-full border px-2 py-0.5 transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              area === option
                ? "border-foreground/30 bg-accent text-accent-foreground"
                : "border-border text-muted-foreground hover:bg-accent/50",
            )}
          >
            {option === "all" ? "Everything" : SOURCE_AREA_LABELS[option]} ({areaCounts[option]})
          </button>
        ))}
        <span className="mx-1 h-4 w-px bg-border" aria-hidden />
        {(["all", "unscanned", "stale"] as CoverageFilter[]).map((option) => (
          <button
            key={option}
            aria-pressed={coverage === option}
            onClick={() => setCoverage(option)}
            className={cn(
              "rounded-full border px-2 py-0.5 capitalize transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              coverage === option
                ? "border-foreground/30 bg-accent text-accent-foreground"
                : "border-border text-muted-foreground hover:bg-accent/50",
            )}
          >
            {option === "all" ? "Any state" : option === "unscanned" ? "Never scanned" : "Changed since"}
          </button>
        ))}
        <label className="ml-auto flex items-center gap-1 text-muted-foreground">
          <Search className="h-3 w-3" aria-hidden />
          <span className="sr-only">Filter by path</span>
          <input
            className="w-40 rounded border bg-background px-2 py-0.5 text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Filter paths…"
          />
        </label>
      </div>

      <div className="flex-1 overflow-auto">
        {refreshing && sources.length === 0 ? (
          <p className="flex items-center justify-center gap-2 p-8 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            Asking the engine what it indexed…
          </p>
        ) : visible.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
            <FileSearch className="h-8 w-8 text-muted-foreground/40" aria-hidden />
            <p>
              {sources.length === 0
                ? "The engine has recorded no indexed files for this project yet."
                : "No files match this filter."}
            </p>
          </div>
        ) : (
          <table className="w-full border-collapse text-xs">
            <caption className="sr-only">
              One row per scan and file: what the engine read in its last index pass, with the
              coverage each scan has of it
            </caption>
            <thead className="sticky top-0 bg-muted/90 text-left">
              <tr>
                <th scope="col" className="px-3 py-2 font-medium">Path</th>
                <th scope="col" className="px-3 py-2 font-medium">Scan</th>
                <th scope="col" className="px-3 py-2 font-medium">Coverage</th>
                <th scope="col" className="px-3 py-2 text-right font-medium">Size</th>
                <th scope="col" className="px-3 py-2 text-right font-medium">Nodes</th>
                <th scope="col" className="px-3 py-2 text-right font-medium">Anchors</th>
                <th scope="col" className="px-3 py-2 font-medium">sha256</th>
                <th scope="col" className="px-3 py-2 font-medium">Indexed</th>
              </tr>
            </thead>
            <tbody>
              {visible.map((source) => (
                <SourceRow
                  key={source.path}
                  source={source}
                  onOpen={() =>
                    workspace &&
                    openPage(
                      source.path.startsWith("/") ? source.path : `${workspace}/${source.path}`,
                    )
                  }
                />
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}

function StatCell({
  label,
  value,
  tone = "muted",
}: {
  label: string
  value: number
  tone?: "muted" | "good" | "warn" | "bad"
}) {
  return (
    <div className="bg-background px-3 py-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd
        className={cn(
          "text-base font-semibold",
          tone === "good" && "text-emerald-600 dark:text-emerald-500",
          tone === "warn" && "text-amber-600 dark:text-amber-500",
          tone === "bad" && "text-destructive",
        )}
      >
        {value}
      </dd>
    </div>
  )
}

function SourceRow({ source, onOpen }: { source: EngineSource; onOpen: () => void }) {
  return (
    <tr className="border-b last:border-b-0 hover:bg-accent/30">
      <th scope="row" className="max-w-[28rem] px-3 py-1.5 text-left font-normal">
        <button
          className="break-all text-left text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={onOpen}
        >
          {source.path}
        </button>
        <span className="ml-2 text-[10px] uppercase tracking-wide text-muted-foreground">
          {SOURCE_AREA_LABELS[sourceArea(source.path)]}
        </span>
      </th>
      <td className="px-3 py-1.5 whitespace-nowrap">
        <span className="rounded-full border border-border px-2 py-0.5 text-[11px] text-muted-foreground">
          {sourceScopeLabel(source.scope)}
        </span>
      </td>
      <td className="px-3 py-1.5">
        {!source.scanned ? (
          <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/40 bg-amber-500/10 px-2 py-0.5 text-[11px] text-amber-700 dark:text-amber-500">
            <XCircle className="h-3 w-3" aria-hidden />
            Never scanned
          </span>
        ) : source.stale ? (
          <span className="inline-flex items-center gap-1 rounded-full border border-amber-500/40 bg-amber-500/10 px-2 py-0.5 text-[11px] text-amber-700 dark:text-amber-500">
            <AlertTriangle className="h-3 w-3" aria-hidden />
            Changed since
          </span>
        ) : (
          <span className="inline-flex items-center gap-1 rounded-full border border-emerald-500/40 bg-emerald-500/10 px-2 py-0.5 text-[11px] text-emerald-700 dark:text-emerald-500">
            <CheckCircle2 className="h-3 w-3" aria-hidden />
            Scanned
          </span>
        )}
      </td>
      <td className="px-3 py-1.5 text-right tabular-nums">{formatBytes(source.bytes)}</td>
      <td className="px-3 py-1.5 text-right tabular-nums">{source.nodes}</td>
      <td className="px-3 py-1.5 text-right tabular-nums">{source.anchors}</td>
      <td className="px-3 py-1.5 font-mono" title={source.sha256 || "no hash recorded"}>
        {source.sha256 ? shortHash(source.sha256) : "—"}
      </td>
      <td className="px-3 py-1.5 whitespace-nowrap">
        {source.indexedAt ? (
          <span title={new Date(source.indexedAt).toLocaleString()}>
            {formatAge(source.indexedAt)}
          </span>
        ) : (
          <span className="text-muted-foreground">never</span>
        )}
      </td>
    </tr>
  )
}
