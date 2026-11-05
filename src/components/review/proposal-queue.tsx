/**
 * The engine's review queue: proposals a model made, waiting for a person.
 *
 * The card shows what the engine actually recorded about the proposal —
 * authority, provenance, source anchors, the engine's own reason and
 * confidence — and the two buttons that decide it. Nothing here writes
 * knowledge: accepting calls the engine, which writes the Markdown page, and
 * rejecting writes nothing at all.
 *
 * The text of a proposal can be edited before it is decided, because the
 * person approving a page is the author of record. What gets written is the
 * edited text, not the model's draft.
 */
import { useCallback, useEffect, useMemo, useState } from "react"
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  FileText,
  Loader2,
  Pencil,
  RefreshCw,
  ShieldQuestion,
  X,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useProposalStore } from "@/stores/proposal-store"
import {
  engineError,
  explainProposal,
  type EngineFinding,
  type EngineNode,
  type EngineProposal,
  type EngineVersion,
} from "@/lib/engine-client"
import { engineNodeHistory } from "@/lib/engine"
import { normalizePath } from "@/lib/path-utils"
import {
  anchorLabel,
  formatAge,
  formatConfidence,
  provenanceLabel,
  severityBadgeTone,
  severityLabel,
  shortHash,
  standingLabel,
} from "@/lib/engine-present"
import { cn } from "@/lib/utils"
import { useAutomationStore } from "@/stores/automation-store"

export function ProposalQueue() {
  const project = useWikiStore((s) => s.project)
  const openPage = useWikiStore((s) => s.openPathInPreview)
  const findings = useEngineStore((s) => s.findings)
  const engineState = useEngineStore((s) => s.state)
  const refreshEngine = useEngineStore((s) => s.refresh)
  const proposals = useProposalStore((s) => s.proposals)
  const loading = useProposalStore((s) => s.loading)
  const error = useProposalStore((s) => s.error)
  const loadedAt = useProposalStore((s) => s.loadedAt)
  const decisions = useProposalStore((s) => s.decisions)
  const load = useProposalStore((s) => s.load)
  const clearDecision = useProposalStore((s) => s.clearDecision)
  const editing = useProposalStore((s) => s.editing)
  const automation = useAutomationStore()

  // `useEngineWorkspace` attaches both stores to the open project; this pane
  // only asks for a fresh read when it is first shown.
  const workspace = project ? project.path : null

  useEffect(() => {
    if (workspace) void load()
  }, [workspace, load])

  // Agents propose while the app is open, and a queue nobody re-reads is a
  // queue that lies by omission. Cheap query, and only while this tab is
  // actually on screen in a focused window.
  useEffect(() => {
    if (!workspace) return
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void load()
    }, 30_000)
    return () => clearInterval(timer)
  }, [workspace, load])

  const decisionIds = useMemo(() => Object.keys(decisions), [decisions])

  return (
    <div className="flex h-full flex-col">
      <div className="flex flex-wrap items-center justify-between gap-2 border-b px-4 py-3">
        <div className="flex items-center gap-2">
          <Button size="sm" disabled={!workspace || loading || automation.busy || editing !== null || proposals.length === 0}
            onClick={() => workspace && void automation.run(workspace, { acceptAll: true })}>
            {automation.busy ? "Saving pages…" : `Accept all (${proposals.length})`}
          </Button>
          <h2 className="text-sm font-semibold">Proposals</h2>
          {proposals.length > 0 && (
            <span
              className="rounded-full bg-primary px-2 py-0.5 text-xs text-primary-foreground"
              aria-label={`${proposals.length} proposals awaiting a decision`}
            >
              {proposals.length}
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground" aria-live="polite">
            {loadedAt ? `Checked ${formatAge(new Date(loadedAt).toISOString())}` : null}
          </span>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs"
            onClick={() => void load({ force: true })}
            disabled={loading || !workspace}
          >
            <RefreshCw className={cn("mr-1 h-3 w-3", loading && "animate-spin")} />
            Refresh
          </Button>
        </div>
      </div>

      <div className="space-y-2 border-b px-4 py-3 text-xs text-muted-foreground">
        <label className="flex items-center gap-2 text-foreground">
          <input type="checkbox" checked={automation.workspace === workspace && automation.settings.acceptProposals}
            disabled={!workspace || automation.busy}
            onChange={(event) => workspace && void automation.run(workspace, {
              settings: { ...automation.settings, acceptProposals: event.target.checked },
            })} />
          Automatically save proposals as pages
        </label>
        <p>New proposals save after a short pause while this wiki is open. Their verification status stays visible.</p>
        <p className="break-all">Pages are saved in {workspace}/wiki</p>
        {automation.error && <p role="alert" className="text-destructive">{automation.error}</p>}
      </div>

      {error && engineState.kind !== "absent-vault" && (
        <div role="alert" className="border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-xs text-destructive">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        {engineState.kind === "absent-vault" ? (
          <div className="flex flex-col items-center gap-2 p-8 text-center text-sm text-muted-foreground">
            <ShieldQuestion className="h-8 w-8 text-muted-foreground/40" aria-hidden />
            <p>{engineState.message}</p>
            <p className="max-w-md text-xs">
              Nothing can be proposed until there is a knowledge base to propose into, and nothing
              here writes one.
            </p>
            <Button variant="outline" size="sm" className="h-7 text-xs" onClick={() => void refreshEngine()}>
              <RefreshCw className="mr-1 h-3 w-3" aria-hidden />
              Check again
            </Button>
          </div>
        ) : engineState.kind === "protocol-mismatch" || engineState.kind === "unreachable" ? (
          <div role="alert" className="flex flex-col items-center gap-2 p-8 text-center text-sm">
            <AlertTriangle className="h-8 w-8 text-destructive/60" aria-hidden />
            <p>{engineState.message}</p>
            <p className="max-w-md text-xs text-muted-foreground">
              The queue is not shown, because an answer this window cannot read must not be rendered
              as if it were the engine&apos;s list.
            </p>
          </div>
        ) : loading && proposals.length === 0 ? (
          <div className="flex items-center justify-center gap-2 p-8 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
            Asking the engine what has been proposed…
          </div>
        ) : proposals.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 p-8 text-center text-sm text-muted-foreground">
            <Check className="h-8 w-8 text-muted-foreground/40" aria-hidden />
            <p>Nothing is waiting for a decision.</p>
            <p className="max-w-md text-xs">
              Proposals appear here when an agent records one through the bridge. Nothing becomes
              permanent until you accept it here.
            </p>
          </div>
        ) : (
          <ul className="flex flex-col gap-3 p-3">
            {proposals.map((proposal) => (
              <li key={proposal.node.id}>
                <ProposalCard
                  proposal={proposal}
                  findings={findings}
                  workspace={workspace}
                  openPage={openPage}
                />
              </li>
            ))}
          </ul>
        )}

        {decisionIds.length > 0 && (
          <section className="border-t px-3 py-3" aria-label="Decisions taken in this session">
            <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Decided in this session
            </h3>
            <ul className="flex flex-col gap-2">
              {decisionIds.map((node) => {
                const decision = decisions[node]
                if (!decision) return null
                return (
                  <li
                    key={node}
                    className={cn(
                      "rounded-md border p-2 text-xs",
                      decision.error ? "border-destructive/40 bg-destructive/5" : "bg-muted/30",
                    )}
                  >
                    {decision.error ? (
                      <div className="flex items-start gap-2">
                        <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-destructive" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="font-medium text-destructive">
                            {decision.action === "accepted" ? "Accepting failed" : "Rejecting failed"}
                          </p>
                          <p className="break-words text-muted-foreground">{decision.error}</p>
                          <p className="mt-1 text-muted-foreground">
                            The proposal is still in the queue; nothing durable changed.
                          </p>
                        </div>
                        <button
                          className="rounded p-0.5 text-muted-foreground hover:bg-muted"
                          onClick={() => clearDecision(node)}
                          aria-label="Dismiss this message"
                        >
                          <X className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    ) : decision.action === "accepted" ? (
                      <div className="flex items-start gap-2">
                        <Check className="mt-0.5 h-3.5 w-3.5 shrink-0 text-emerald-600" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="font-medium">
                            {decision.alreadyAccepted
                              ? "Already had a page; nothing was rewritten."
                              : "Accepted and written to the vault."}
                          </p>
                          {decision.page && (
                            <button
                              className="mt-0.5 break-all text-left text-primary underline-offset-2 hover:underline"
                              onClick={() =>
                                openPage(`${workspace?.replace(/\/+$/, "")}/${decision.page?.path}`)
                              }
                            >
                              {decision.page.path}
                            </button>
                          )}
                          <p className="mt-1 text-muted-foreground">
                            {decision.relationshipsWritten ?? 0} relationship
                            {(decision.relationshipsWritten ?? 0) === 1 ? "" : "s"} written
                            {(decision.relationshipsLeftLocal?.length ?? 0) > 0 &&
                              `; ${decision.relationshipsLeftLocal?.length} left in the index because they point at something unaccepted`}
                            . Accepting is not verifying: the page keeps the authority it arrived
                            with.
                          </p>
                          {decision.node && workspace && (
                            <RecordedIdentity
                              workspace={workspace}
                              node={decision.node}
                              pageHash={decision.page?.contentHash ?? ""}
                            />
                          )}
                        </div>
                        <button
                          className="rounded p-0.5 text-muted-foreground hover:bg-muted"
                          onClick={() => clearDecision(node)}
                          aria-label="Dismiss this message"
                        >
                          <X className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    ) : (
                      <div className="flex items-start gap-2">
                        <X className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
                        <div className="min-w-0 flex-1">
                          <p className="font-medium">Rejected. Nothing was written to the vault.</p>
                          <p className="mt-0.5 text-muted-foreground">
                            The engine will not offer it again. The record of the decision stays in
                            the journal.
                          </p>
                        </div>
                        <button
                          className="rounded p-0.5 text-muted-foreground hover:bg-muted"
                          onClick={() => clearDecision(node)}
                          aria-label="Dismiss this message"
                        >
                          <X className="h-3.5 w-3.5" />
                        </button>
                      </div>
                    )}
                  </li>
                )
              })}
            </ul>
          </section>
        )}
      </div>
    </div>
  )
}

/**
 * What the engine recorded about the page an acceptance produced: the node's
 * provenance and standing, the hash of the Markdown written, and the versions
 * it has kept. Loaded from the engine after the fact, never assumed.
 */
function RecordedIdentity({
  workspace,
  node,
  pageHash,
}: {
  workspace: string
  node: EngineNode
  pageHash: string
}) {
  const [open, setOpen] = useState(false)
  const [history, setHistory] = useState<EngineVersion[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setHistory(await engineNodeHistory(workspace, node.id))
    } catch (cause) {
      setError(engineError(cause).message)
    } finally {
      setLoading(false)
    }
  }, [workspace, node.id])

  return (
    <div className="mt-1">
      <p className="text-muted-foreground">
        Written as {node.type} · {standingLabel(node)} · {provenanceLabel(node)}
        {pageHash ? ` · sha256 ${shortHash(pageHash)}` : ""}
      </p>
      <button
        className="mt-0.5 text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
        onClick={() => {
          const next = !open
          setOpen(next)
          if (next && history === null) void load()
        }}
        aria-expanded={open}
      >
        {open ? "Hide recorded history" : "Show recorded history"}
      </button>
      {open && (
        <div className="mt-1">
          {loading && (
            <p className="flex items-center gap-1 text-muted-foreground">
              <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
              Reading versions…
            </p>
          )}
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
          {history && history.length === 0 && (
            <p className="text-muted-foreground">The engine keeps no versions for this node yet.</p>
          )}
          {history && history.length > 0 && (
            <ol className="space-y-1">
              {history.map((version) => (
                <li key={`${version.version}-${version.recordedAt}`} className="text-muted-foreground">
                  v{version.version} · {formatAge(version.recordedAt)} · {version.title}
                  {version.reason ? ` — ${version.reason}` : ""}
                </li>
              ))}
            </ol>
          )}
        </div>
      )}
    </div>
  )
}

function ProposalCard({
  proposal,
  findings,
  workspace,
  openPage,
}: {
  proposal: EngineProposal
  findings: EngineFinding[]
  workspace: string | null
  openPage: (path: string) => void
}) {
  const node = proposal.node
  const explanation = explainProposal(node.id, findings)
  const [showEvidence, setShowEvidence] = useState(false)
  /**
   * Accepting without a recorded anchor is allowed — the engine permits it and
   * the decision is the person's — but it takes one deliberate act more than
   * accepting a proposal that rests on a source.
   */
  const [acceptedWithoutEvidence, setAcceptedWithoutEvidence] = useState(false)

  const editing = useProposalStore((s) => s.editing === node.id)
  const draft = useProposalStore((s) => s.drafts[node.id])
  const busy = useProposalStore((s) => s.busy[node.id])
  const beginEdit = useProposalStore((s) => s.beginEdit)
  const cancelEdit = useProposalStore((s) => s.cancelEdit)
  const setDraft = useProposalStore((s) => s.setDraft)
  const resetDraft = useProposalStore((s) => s.resetDraft)
  const accept = useProposalStore((s) => s.accept)
  const reject = useProposalStore((s) => s.reject)

  const text = draft ?? { title: node.title, summary: node.summary, content: node.content }
  const dirty =
    text.title !== node.title || text.summary !== node.summary || text.content !== node.content

  const evidence = proposal.evidence
  const needsAcknowledgement = evidence.length === 0
  const canAccept =
    !busy &&
    (!needsAcknowledgement || acceptedWithoutEvidence) &&
    (!dirty || text.title.trim().length > 0)

  const handleAccept = useCallback(() => {
    void accept(node.id)
  }, [accept, node.id])

  const handleReject = useCallback(() => {
    void reject(node.id, "rejected from the desktop review queue")
  }, [reject, node.id])

  return (
    <article
      className="rounded-lg border bg-card p-3 text-sm shadow-sm"
      aria-label={`Proposal: ${node.title}`}
    >
      <header className="mb-2 flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="font-medium">{node.title || "(untitled proposal)"}</h3>
          <p className="mt-0.5 text-xs text-muted-foreground">
            {node.type} · {standingLabel(node)}
          </p>
        </div>
        {explanation.severity && (
          <span
            className={cn(
              "rounded-full border px-2 py-0.5 text-[11px]",
              severityBadgeTone(explanation.severity),
            )}
            title="Severity the engine attached to this proposal"
          >
            {severityLabel(explanation.severity)}
          </span>
        )}
      </header>

      <dl className="mb-2 grid grid-cols-[max-content_1fr] gap-x-3 gap-y-1 text-xs">
        <dt className="text-muted-foreground">Attribution</dt>
        <dd className="break-words">{provenanceLabel(node)}</dd>
        <dt className="text-muted-foreground">Confidence</dt>
        <dd>
          {formatConfidence(explanation.confidence)}
          {explanation.confidence === null && (
            <span className="text-muted-foreground">
              {" "}
              — bridge v1 carries it on the engine&rsquo;s review finding, not on the proposal
            </span>
          )}
        </dd>
        <dt className="text-muted-foreground">Engine&rsquo;s reason</dt>
        <dd className="break-words">
          {explanation.reason ?? (
            <span className="text-muted-foreground">
              The engine did not report a reason for this proposal.
            </span>
          )}
        </dd>
        <dt className="text-muted-foreground">Recorded</dt>
        <dd>{formatAge(node.createdAt)}</dd>
      </dl>

      {editing ? (
        <form
          className="mb-3 space-y-2"
          onSubmit={(event) => {
            event.preventDefault()
            if (canAccept) handleAccept()
          }}
        >
          <div>
            <label className="text-xs font-medium" htmlFor={`proposal-title-${node.id}`}>
              Page title
            </label>
            <input
              id={`proposal-title-${node.id}`}
              className="mt-1 w-full rounded border bg-background px-2 py-1 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={text.title}
              onChange={(event) => setDraft(node.id, { title: event.target.value })}
            />
          </div>
          <div>
            <label className="text-xs font-medium" htmlFor={`proposal-summary-${node.id}`}>
              Summary
            </label>
            <textarea
              id={`proposal-summary-${node.id}`}
              rows={2}
              className="mt-1 w-full rounded border bg-background px-2 py-1 text-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={text.summary}
              onChange={(event) => setDraft(node.id, { summary: event.target.value })}
            />
          </div>
          <div>
            <label className="text-xs font-medium" htmlFor={`proposal-content-${node.id}`}>
              Page body (Markdown)
            </label>
            <textarea
              id={`proposal-content-${node.id}`}
              rows={10}
              className="mt-1 w-full rounded border bg-background px-2 py-1 font-mono text-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={text.content}
              onChange={(event) => setDraft(node.id, { content: event.target.value })}
            />
          </div>
          <p className="text-xs text-muted-foreground">
            Accepting writes this text, not the model&rsquo;s draft.
            {dirty && " (Edited)"}
          </p>
        </form>
      ) : (
        <>
          {node.summary && <p className="mb-2 text-xs text-muted-foreground">{node.summary}</p>}
          {node.content && (
            <pre className="mb-2 max-h-40 overflow-auto whitespace-pre-wrap rounded bg-muted/40 p-2 text-xs">
              {node.content}
            </pre>
          )}
        </>
      )}

      {proposal.relationships.length > 0 && (
        <div className="mb-2 text-xs">
          <span className="text-muted-foreground">Links this proposal would add: </span>
          {proposal.relationships.map((relationship) => (
            <span key={relationship.edgeId || relationship.id} className="mr-2 inline-block">
              {relationship.outbound ? "→" : "←"} {relationship.edgeType}{" "}
              <span className="text-muted-foreground">{relationship.title || relationship.id}</span>
              {!relationship.established && (
                <span className="ml-1 text-amber-600 dark:text-amber-500">(not established)</span>
              )}
            </span>
          ))}
        </div>
      )}

      <div className="mb-3">
        <button
          className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground"
          onClick={() => setShowEvidence((value) => !value)}
          aria-expanded={showEvidence}
        >
          {showEvidence ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          Evidence ({evidence.length})
        </button>
        {showEvidence && (
          <div className="mt-1">
            {evidence.length === 0 ? (
              <p className="text-xs text-amber-600 dark:text-amber-500">
                The engine recorded no source evidence for this proposal. It can still be accepted,
                but only after saying so below.
              </p>
            ) : (
              <ul className="space-y-1">
                {evidence.map((item) => (
                  <li key={item.id} className="rounded bg-muted/40 p-2 text-xs">
                    <button
                      className="flex items-center gap-1 break-all text-left text-primary underline-offset-2 hover:underline disabled:text-muted-foreground disabled:no-underline"
                      onClick={() => {
                        const file = item.anchor?.file
                        if (!file || !workspace) return
                        const root = normalizePath(workspace).replace(/\/+$/, "")
                        openPage(file.startsWith("/") ? file : `${root}/${file}`)
                      }}
                      disabled={!item.anchor?.file || !workspace}
                    >
                      <FileText className="h-3 w-3 shrink-0" aria-hidden />
                      {anchorLabel(item)}
                    </button>
                    <span className="ml-1 text-muted-foreground">{item.kind}</span>
                    {item.excerpt && (
                      <blockquote className="mt-1 whitespace-pre-wrap border-l-2 pl-2 text-muted-foreground">
                        {item.excerpt}
                      </blockquote>
                    )}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Button
          size="sm"
          className="h-7 text-xs"
          disabled={!canAccept}
          onClick={handleAccept}
          title={
            needsAcknowledgement && !acceptedWithoutEvidence
              ? "Tick the acknowledgement first: this proposal rests on no recorded source evidence"
              : "Write this proposal to the vault as a Markdown page"
          }
        >
          {busy === "accept" ? (
            <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden />
          ) : (
            <Check className="mr-1 h-3 w-3" aria-hidden />
          )}
          Accept and write page
        </Button>
        <Button
          size="sm"
          variant="outline"
          className="h-7 text-xs"
          onClick={() => (editing ? cancelEdit(node.id) : beginEdit(node.id))}
          disabled={busy !== undefined}
        >
          <Pencil className="mr-1 h-3 w-3" aria-hidden />
          {editing ? "Stop editing" : "Edit before accepting"}
        </Button>
        {editing && dirty && (
          <Button
            size="sm"
            variant="ghost"
            className="h-7 text-xs"
            onClick={() => resetDraft(node.id)}
            disabled={busy !== undefined}
          >
            Reset to proposal
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          className="h-7 text-xs text-destructive hover:text-destructive"
          onClick={handleReject}
          disabled={busy !== undefined}
          title="Decline this proposal. Nothing is written to the vault."
        >
          {busy === "reject" ? (
            <Loader2 className="mr-1 h-3 w-3 animate-spin" aria-hidden />
          ) : (
            <X className="mr-1 h-3 w-3" aria-hidden />
          )}
          Reject
        </Button>
        <span className="flex items-center gap-1 text-[11px] text-muted-foreground">
          <ShieldQuestion className="h-3 w-3" aria-hidden />
          Only accepting here writes anything.
        </span>
      </div>
      {needsAcknowledgement && (
        <label className="mt-2 flex items-start gap-2 text-xs text-amber-700 dark:text-amber-500">
          <input
            type="checkbox"
            className="mt-0.5 h-3.5 w-3.5"
            checked={acceptedWithoutEvidence}
            onChange={(event) => setAcceptedWithoutEvidence(event.target.checked)}
            disabled={busy !== undefined}
          />
          <span>
            The engine recorded no source evidence for this proposal. I have read it and I am
            accepting it anyway, as unverified knowledge.
          </span>
        </label>
      )}
      {busy !== undefined && (
        <p className="mt-2 flex items-center gap-1 text-xs text-muted-foreground" aria-live="polite">
          <Loader2 className="h-3 w-3 animate-spin" aria-hidden />
          {busy === "accept"
            ? "Asking the engine to write the page…"
            : "Asking the engine to reject…"}
        </p>
      )}
    </article>
  )
}
