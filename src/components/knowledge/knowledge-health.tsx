/**
 * Knowledge health, as the engine reports it.
 *
 * Authority, stale evidence, duplicates, contradictions, superseded claims,
 * unsupported rules and incomplete scans are all review findings the engine
 * produced by sweeping the knowledge base. This view groups them, shows what
 * each one rests on — the nodes, the source anchors, the engine's confidence —
 * and offers the two honest actions: re-read the engine, or ask it to rebuild
 * the derived index.
 *
 * Nothing is recomputed here. If the engine did not report it, it is not shown.
 */
import { useCallback, useEffect, useMemo, useState } from "react"
import {
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  Database,
  ExternalLink,
  Info,
  Loader2,
  RefreshCw,
  ShieldAlert,
  Wrench,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { engineGraphPath, engineNodeDetail } from "@/lib/engine"
import {
  engineError,
  type EngineFinding,
  type EngineGraphPath,
  type EngineNodeDetail,
} from "@/lib/engine-client"
import {
  anchorLabel,
  describeIndex,
  formatAge,
  formatConfidence,
  provenanceLabel,
  standingLabel,
  groupFindings,
  severityBadgeTone,
  severityLabel,
  severityTone,
} from "@/lib/engine-present"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"

type SeverityFilter = "all" | "blocking" | "warning" | "info"

/** How long findings are trusted before opening this surface re-reads them. */
const FINDINGS_FRESH_MS = 15_000

export function KnowledgeHealth() {
  const project = useWikiStore((s) => s.project)
  const openPage = useWikiStore((s) => s.openPathInPreview)
  const status = useEngineStore((s) => s.status)
  const state = useEngineStore((s) => s.state)
  const findings = useEngineStore((s) => s.findings)
  const reviewError = useEngineStore((s) => s.reviewError)
  const indexReport = useEngineStore((s) => s.indexReport)
  const indexError = useEngineStore((s) => s.indexError)
  const refreshing = useEngineStore((s) => s.refreshing)
  const findingsLoadedAt = useEngineStore((s) => s.findingsLoadedAt)
  const loadFindings = useEngineStore((s) => s.loadFindings)
  const rebuildIndex = useEngineStore((s) => s.rebuildIndex)
  const clearIndexReport = useEngineStore((s) => s.clearIndexReport)

  const [severity, setSeverity] = useState<SeverityFilter>("all")
  const [kindFilter, setKindFilter] = useState<string | null>(null)
  const [rebuilding, setRebuilding] = useState(false)
  const [rereading, setRereading] = useState(false)

  const workspace = project ? normalizePath(project.path).replace(/\/+$/, "") : null

  const groups = useMemo(() => groupFindings(findings), [findings])
  const visibleGroups = useMemo(() => {
    const filterSeverity = (finding: EngineFinding) => {
      if (severity === "all") return true
      if (severity === "blocking") return finding.blocking
      return finding.severity === severity
    }
    const filterKind = (finding: EngineFinding) => kindFilter === null || finding.kind === kindFilter
    return groups
      .map((group) => ({
        ...group,
        findings: group.findings.filter((finding) => filterSeverity(finding) && filterKind(finding)),
      }))
      .filter((group) => group.findings.length > 0)
  }, [groups, severity, kindFilter])

  const blockingCount = findings.filter((finding) => finding.blocking).length
  const shownCount = visibleGroups.reduce((sum, group) => sum + group.findings.length, 0)

  // Findings are a sweep over the whole knowledge base, so they are read when
  // this surface is opened and are kept for a moment rather than re-read on
  // every vault write elsewhere in the app.
  useEffect(() => {
    if (state.kind === "absent-vault" || state.kind === "protocol-mismatch") return
    if (findingsLoadedAt !== null && Date.now() - findingsLoadedAt < FINDINGS_FRESH_MS) return
    void loadFindings()
  }, [findingsLoadedAt, state.kind, loadFindings])

  const handleReread = useCallback(async () => {
    setRereading(true)
    try {
      await loadFindings()
    } finally {
      setRereading(false)
    }
  }, [loadFindings])

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
        <div className="flex flex-wrap items-center gap-3 text-sm">
          <h2 className="font-semibold">Knowledge health</h2>
          {status && (
            <span className="text-xs text-muted-foreground">
              {status.nodes} nodes · {status.edges} edges · {status.vaultPages} pages ·{" "}
              {status.indexedAt ? `indexed ${formatAge(status.indexedAt)}` : "never indexed"}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs"
            onClick={() => void handleReread()}
            disabled={rereading}
          >
            <RefreshCw className={cn("mr-1 h-3 w-3", rereading && "animate-spin")} aria-hidden />
            {rereading ? "Reading…" : "Re-read findings"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs"
            onClick={() => void handleRebuild()}
            disabled={rebuilding || state.kind === "protocol-mismatch"}
            title="Ask the engine to reconcile the derived index against the vault"
          >
            <Wrench className="mr-1 h-3 w-3" aria-hidden />
            {rebuilding ? "Rebuilding…" : "Rebuild index"}
          </Button>
        </div>
      </div>

      {blockingCount > 0 && (
        <div
          role="status"
          className="flex items-center gap-2 border-b border-destructive/40 bg-destructive/5 px-4 py-2 text-xs text-destructive"
        >
          <ShieldAlert className="h-3.5 w-3.5" aria-hidden />
          {blockingCount} finding{blockingCount === 1 ? "" : "s"} block a clean bill of health. The
          engine will still answer questions; it is saying these need a person.
        </div>
      )}

      {(indexReport || indexError) && (
        <div className="border-b bg-muted/30 px-4 py-2 text-xs" aria-live="polite">
          {indexReport && (
            <div className="flex flex-wrap items-start gap-2">
              <Database className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <div className="min-w-0 flex-1">
                <p>{describeIndex(indexReport)}</p>
                {indexReport.skipped.length > 0 && (
                  <details className="mt-1">
                    <summary className="cursor-pointer text-muted-foreground">
                      {indexReport.skipped.length} skipped path
                      {indexReport.skipped.length === 1 ? "" : "s"}
                    </summary>
                    <ul className="mt-1 list-inside list-disc break-all font-mono text-[11px] text-muted-foreground">
                      {indexReport.skipped.map((path) => (
                        <li key={path}>{path}</li>
                      ))}
                    </ul>
                  </details>
                )}
              </div>
              <button
                className="rounded px-1 text-muted-foreground hover:bg-muted"
                onClick={clearIndexReport}
                aria-label="Dismiss the index report"
              >
                ×
              </button>
            </div>
          )}
          {indexError && (
            <p role="alert" className="text-destructive">
              The index rebuild failed: {indexError}
            </p>
          )}
        </div>
      )}

      {reviewError && (
        <p role="alert" className="border-b border-destructive/40 bg-destructive/5 px-4 py-2 text-xs text-destructive">
          The engine could not be asked for findings: {reviewError}
        </p>
      )}

      {findings.length > 0 && (
        <div className="flex flex-wrap items-center gap-2 border-b bg-muted/20 px-4 py-2 text-xs">
          <span className="text-muted-foreground">Severity</span>
          {(["all", "blocking", "warning", "info"] as SeverityFilter[]).map((option) => (
            <button
              key={option}
              aria-pressed={severity === option}
              onClick={() => setSeverity(option)}
              className={cn(
                "rounded-full border px-2 py-0.5 capitalize transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                severity === option
                  ? "border-foreground/30 bg-accent text-accent-foreground"
                  : "border-border text-muted-foreground hover:bg-accent/50",
              )}
            >
              {option === "all" ? "All" : severityLabel(option)}
              {option === "blocking" && blockingCount > 0 ? ` (${blockingCount})` : ""}
            </button>
          ))}
          {kindFilter && (
            <button
              className="rounded-full border border-border px-2 py-0.5 text-muted-foreground hover:bg-accent/50"
              onClick={() => setKindFilter(null)}
              aria-label="Clear the kind filter"
            >
              kind: {kindFilter} ×
            </button>
          )}
          <span className="ml-auto text-muted-foreground">
            {shownCount} of {findings.length} shown
          </span>
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-3">
        {state.kind === "loading" || (refreshing && findings.length === 0) ? (
          <p className="flex items-center justify-center gap-2 p-8 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            Asking the engine to sweep the knowledge base…
          </p>
        ) : findings.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
            <Info className="h-8 w-8 text-muted-foreground/40" aria-hidden />
            <p>The engine reported nothing that needs a decision.</p>
            <p className="max-w-md text-xs">
              That means no contradictions, stale anchors, duplicates, unsupported rules or
              incomplete scans in what it indexed — not that the wiki is true.
            </p>
          </div>
        ) : visibleGroups.length === 0 ? (
          <p className="p-8 text-center text-sm text-muted-foreground">
            No findings match this filter.
          </p>
        ) : (
          <div className="flex flex-col gap-4">
            {visibleGroups.map((group) => (
              <section key={group.kind} aria-label={group.label}>
                <header className="mb-1 flex flex-wrap items-center gap-2">
                  <button
                    className="text-sm font-medium hover:underline"
                    onClick={() => setKindFilter(kindFilter === group.kind ? null : group.kind)}
                    aria-pressed={kindFilter === group.kind}
                  >
                    {group.label} ({group.findings.length})
                  </button>
                  {group.blockingCount > 0 && (
                    <span className={cn("text-xs", severityTone("blocking"))}>
                      {group.blockingCount} blocking
                    </span>
                  )}
                </header>
                <p className="mb-2 text-xs text-muted-foreground">{group.meaning}</p>
                <ul className="flex flex-col gap-2">
                  {group.findings.map((finding) => (
                    <li key={finding.id}>
                      <FindingCard
                        finding={finding}
                        workspace={workspace}
                        openPage={openPage}
                      />
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * The nodes a finding is about, with what the engine holds about each.
 *
 * A finding names ids; a person needs the claims. `open` gives the node, its
 * authority and status, its provenance, its anchors and its recorded versions
 * in one answer, so the card can show what is actually in dispute.
 */
type DetailState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "loaded"; details: EngineNodeDetail[]; failed: { node: string; message: string }[] }
  | { kind: "failed"; message: string }

/** Most findings are about one or two claims; more than three is a list, not a card. */
const NODE_DETAIL_LIMIT = 3

function FindingCard({
  finding,
  workspace,
  openPage,
}: {
  finding: EngineFinding
  workspace: string | null
  openPage: (path: string) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const [details, setDetails] = useState<DetailState>({ kind: "idle" })
  const [connection, setConnection] = useState<EngineGraphPath | null>(null)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const [checking, setChecking] = useState(false)

  const loadDetails = useCallback(async () => {
    if (!workspace || finding.nodes.length === 0) return
    setDetails({ kind: "loading" })
    const wanted = finding.nodes.slice(0, NODE_DETAIL_LIMIT)
    const settled = await Promise.all(
      wanted.map(async (node) => {
        try {
          return { ok: true as const, detail: await engineNodeDetail(workspace, node) }
        } catch (cause) {
          return { ok: false as const, node, message: engineError(cause).message }
        }
      }),
    )
    const loaded = settled.flatMap((entry) => (entry.ok ? [entry.detail] : []))
    const failed = settled.flatMap((entry) =>
      entry.ok ? [] : [{ node: entry.node, message: entry.message }],
    )
    if (loaded.length === 0 && failed.length > 0) {
      setDetails({ kind: "failed", message: failed[0].message })
      return
    }
    setDetails({ kind: "loaded", details: loaded, failed })
  }, [workspace, finding.nodes])

  const toggle = useCallback(() => {
    setExpanded((value) => {
      const next = !value
      if (next && details.kind === "idle") void loadDetails()
      return next
    })
  }, [details.kind, loadDetails])

  const checkConnection = useCallback(async () => {
    if (!workspace || finding.nodes.length < 2) return
    setChecking(true)
    setConnectionError(null)
    try {
      setConnection(await engineGraphPath(workspace, finding.nodes[0], finding.nodes[1]))
    } catch (cause) {
      setConnection(null)
      setConnectionError(engineError(cause).message)
    } finally {
      setChecking(false)
    }
  }, [workspace, finding.nodes])

  const openPath = useCallback(
    (path: string) => {
      if (!workspace) return
      openPage(path.startsWith("/") ? path : `${workspace}/${path}`)
    },
    [openPage, workspace],
  )

  return (
    <article
      className={cn(
        "rounded-lg border p-3 text-sm",
        finding.blocking ? "border-destructive/30" : "border-border",
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <button
          className="flex min-w-0 flex-1 items-start gap-2 text-left"
          onClick={toggle}
          aria-expanded={expanded}
        >
          {expanded ? (
            <ChevronDown className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          ) : (
            <ChevronRight className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
          )}
          <span className="min-w-0">
            <span className="font-medium">{finding.title}</span>
            <span className="mt-0.5 block text-xs text-muted-foreground">{finding.kind}</span>
          </span>
        </button>
        <span
          className={cn(
            "shrink-0 rounded-full border px-2 py-0.5 text-[11px]",
            severityBadgeTone(finding.severity),
          )}
        >
          {severityLabel(finding.severity)}
        </span>
      </div>

      <p className="mt-2 whitespace-pre-wrap text-xs text-muted-foreground">{finding.detail}</p>

      <dl className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <div className="flex gap-1">
          <dt>Confidence</dt>
          <dd className="text-foreground">{formatConfidence(finding.confidence)}</dd>
        </div>
        <div className="flex gap-1">
          <dt>Nodes</dt>
          <dd className="break-all font-mono text-foreground">{finding.nodes.join(", ") || "none"}</dd>
        </div>
        {finding.sourcePath && (
          <div className="flex items-center gap-1">
            <dt>Source</dt>
            <dd>
              <button
                className="break-all font-mono text-primary underline-offset-2 hover:underline"
                onClick={() => openPath(finding.sourcePath as string)}
              >
                {finding.sourcePath}
              </button>
            </dd>
          </div>
        )}
        {finding.blocking && (
          <div className="flex items-center gap-1 text-destructive">
            <AlertTriangle className="h-3 w-3" aria-hidden />
            <span>blocks a clean bill of health</span>
          </div>
        )}
      </dl>

      {expanded && (
        <div className="mt-3 space-y-3 border-t pt-2">
          <div>
            <h4 className="text-xs font-medium">
              Claims in this finding ({finding.nodes.length})
            </h4>
            {finding.nodes.length === 0 ? (
              <p className="text-xs text-muted-foreground">
                The finding names no claim. That is itself the finding when the problem is a scan
                that could not cover everything.
              </p>
            ) : details.kind === "loading" ? (
              <p className="flex items-center gap-1 text-xs text-muted-foreground">
                <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
                Reading the claims from the engine…
              </p>
            ) : details.kind === "failed" ? (
              <p role="alert" className="text-xs text-destructive">
                {details.message}
              </p>
            ) : details.kind === "loaded" ? (
              <ul className="mt-1 space-y-2">
                {details.details.map((detail) => (
                  <li key={detail.node.id} className="rounded bg-muted/40 p-2 text-xs">
                    <div className="flex flex-wrap items-baseline justify-between gap-2">
                      <span className="font-medium">{detail.node.title || detail.node.id}</span>
                      <span className="text-muted-foreground">{standingLabel(detail.node)}</span>
                    </div>
                    <p className="mt-0.5 text-muted-foreground">
                      {detail.node.type} · {provenanceLabel(detail.node)}
                    </p>
                    {detail.node.summary && (
                      <p className="mt-1 whitespace-pre-wrap">{detail.node.summary}</p>
                    )}
                    {detail.page && (
                      <button
                        className="mt-1 break-all text-left font-mono text-primary underline-offset-2 hover:underline"
                        onClick={() => openPath(detail.page?.path ?? "")}
                      >
                        {detail.page.path}
                      </button>
                    )}
                    {detail.evidence.length === 0 ? (
                      <p className="mt-1 text-amber-700 dark:text-amber-500">
                        No evidence record for this claim.
                      </p>
                    ) : (
                      <ul className="mt-1 space-y-1">
                        {detail.evidence.map((record) => {
                          const named = finding.evidence.includes(record.id)
                          return (
                            <li
                              key={record.id}
                              className={cn(
                                "rounded p-1.5",
                                named ? "bg-background" : "bg-muted/30 opacity-70",
                              )}
                            >
                              <div className="flex flex-wrap items-center gap-2">
                                <span className="font-mono">{record.id}</span>
                                <span className="text-muted-foreground">{record.kind}</span>
                                {named && (
                                  <span className="rounded bg-primary/10 px-1 text-[10px] text-primary">
                                    named by this finding
                                  </span>
                                )}
                              </div>
                              {record.anchor && (
                                <button
                                  className="mt-0.5 block break-all text-left font-mono text-primary underline-offset-2 hover:underline"
                                  onClick={() => openPath(record.anchor?.file ?? "")}
                                >
                                  <ExternalLink className="mr-1 inline h-3 w-3" aria-hidden />
                                  {anchorLabel(record)}
                                </button>
                              )}
                              {record.excerpt && (
                                <blockquote className="mt-1 whitespace-pre-wrap border-l-2 pl-2 text-muted-foreground">
                                  {record.excerpt}
                                </blockquote>
                              )}
                            </li>
                          )
                        })}
                      </ul>
                    )}
                    {detail.history.length > 0 && (
                      <p className="mt-1 text-muted-foreground">
                        {detail.history.length} recorded version
                        {detail.history.length === 1 ? "" : "s"}; latest{" "}
                        {formatAge(detail.history[detail.history.length - 1].recordedAt)}
                        {detail.history[detail.history.length - 1].reason
                          ? ` — ${detail.history[detail.history.length - 1].reason}`
                          : ""}
                      </p>
                    )}
                  </li>
                ))}
                {finding.nodes.length > details.details.length && (
                  <li className="text-xs text-muted-foreground">
                    Also named:{" "}
                    <span className="font-mono">
                      {finding.nodes.slice(NODE_DETAIL_LIMIT).join(", ")}
                    </span>
                  </li>
                )}
                {details.failed.map((failure) => (
                  <li key={failure.node} role="alert" className="text-xs text-destructive">
                    Could not read <span className="font-mono">{failure.node}</span>:{" "}
                    {failure.message}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>

          {finding.nodes.length > 1 && (
            <div>
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs"
                onClick={() => void checkConnection()}
                disabled={checking}
              >
                {checking ? (
                  <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden />
                ) : (
                  <ChevronRight className="mr-1 h-3 w-3" aria-hidden />
                )}
                How do these two connect?
              </Button>
              {connectionError && (
                <p role="alert" className="mt-1 text-xs text-destructive">
                  {connectionError}
                </p>
              )}
              {connection && (
                <p className="mt-1 text-xs text-muted-foreground">
                  {connection.found ? (
                    <>
                      {connection.steps.length} hop{connection.steps.length === 1 ? "" : "s"}:{" "}
                      <span className="font-mono">
                        {connection.steps
                          .map((step) => `${step.from} —${step.edgeType}→`)
                          .join(" ")}
                      </span>
                    </>
                  ) : (
                    "The engine found no path between them within its budget, so the contradiction is between claims that are not otherwise linked."
                  )}
                </p>
              )}
            </div>
          )}
        </div>
      )}
    </article>
  )
}
