/**
 * Turning engine facts into labels.
 *
 * Nothing here decides anything about knowledge: it maps the engine's closed
 * vocabularies (`kind`, `severity`, `authority`, `status`) to words a person
 * reads, and summarizes what the engine already reported — coverage, groups,
 * anchor labels. Anything the engine did not say stays visibly unsaid.
 */
import type {
  EngineEvidence,
  EngineFinding,
  EngineIndexReport,
  EngineNode,
  EngineSource,
} from "@/lib/engine-client"

export interface KindPresentation {
  label: string
  meaning: string
}

/**
 * The engine's review kinds, as a person reads them. A kind added by a newer
 * engine is shown under its own name rather than hidden or mislabelled.
 */
const KIND_PRESENTATIONS: Record<string, KindPresentation> = {
  contradiction: {
    label: "Contradictions",
    meaning: "Two live claims cannot both hold; a person has to decide which one stands.",
  },
  dispute: {
    label: "Disputed claims",
    meaning: "Several live claims describe one subject; one governs and the rest are shadowed.",
  },
  stale_anchor: {
    label: "Stale evidence",
    meaning: "The source a claim rests on moved or changed, so the anchor no longer resolves.",
  },
  stale: {
    label: "Stale knowledge",
    meaning: "Its source moved or changed; the claim may still hold, but nobody has rechecked it.",
  },
  unsupported_rule: {
    label: "Unsupported rules",
    meaning: "A rule or decision resting on no evidence at all.",
  },
  unsupported: {
    label: "Unsupported claims",
    meaning: "A claim that asserts something without evidence behind it.",
  },
  unreviewed_proposal: {
    label: "Proposals awaiting a decision",
    meaning: "A model proposed something; nothing becomes permanent until a person accepts it.",
  },
  duplicate: {
    label: "Duplicates",
    meaning: "Two nodes describe the same thing.",
  },
  near_duplicate: {
    label: "Near duplicates",
    meaning: "Two nodes are close enough that one is probably redundant.",
  },
  superseded: {
    label: "Superseded knowledge",
    meaning: "A newer claim replaced this one; it is kept for history, not for use.",
  },
  orphan: {
    label: "Orphans",
    meaning: "Nothing links to it, so only a literal search will find it.",
  },
  incomplete_scan: {
    label: "Incomplete scans",
    meaning: "An index pass could not cover everything, so the numbers are a floor.",
  },
}

export function kindPresentation(kind: string): KindPresentation {
  return (
    KIND_PRESENTATIONS[kind] ?? {
      label: kind.replace(/_/g, " ").replace(/^./, (first) => first.toUpperCase()),
      meaning: "This engine reported a kind this window does not recognize yet.",
    }
  )
}

const SEVERITY_LABELS: Record<string, string> = {
  blocking: "Blocking",
  warning: "Warning",
  info: "For information",
}

export function severityLabel(severity: string): string {
  return SEVERITY_LABELS[severity] ?? severity
}

/** Tailwind text colour for a severity; unknown severities stay neutral. */
export function severityTone(severity: string): string {
  switch (severity) {
    case "blocking":
      return "text-destructive"
    case "warning":
      return "text-amber-600 dark:text-amber-500"
    case "info":
      return "text-muted-foreground"
    default:
      return "text-muted-foreground"
  }
}

/** Background/border treatment for a severity badge. */
export function severityBadgeTone(severity: string): string {
  switch (severity) {
    case "blocking":
      return "border-destructive/40 bg-destructive/10 text-destructive"
    case "warning":
      return "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-500"
    case "info":
      return "border-border bg-muted/50 text-muted-foreground"
    default:
      return "border-border bg-muted/50 text-muted-foreground"
  }
}

export interface FindingGroup {
  kind: string
  label: string
  meaning: string
  findings: EngineFinding[]
  blockingCount: number
}

/**
 * Findings in the order a reader should meet them: blocking groups first, then
 * by how many are in each. The engine's own order is kept inside a group, so
 * two runs over an unchanged base read the same.
 */
export function groupFindings(findings: readonly EngineFinding[]): FindingGroup[] {
  const groups = new Map<string, EngineFinding[]>()
  for (const finding of findings) {
    const existing = groups.get(finding.kind)
    if (existing) existing.push(finding)
    else groups.set(finding.kind, [finding])
  }
  return [...groups.entries()]
    .map(([kind, kindFindings]) => ({
      kind,
      ...kindPresentation(kind),
      findings: kindFindings,
      blockingCount: kindFindings.filter((finding) => finding.blocking).length,
    }))
    .sort((a, b) => {
      if (a.blockingCount !== b.blockingCount) return b.blockingCount - a.blockingCount
      if (a.findings.length !== b.findings.length) return b.findings.length - a.findings.length
      return a.label.localeCompare(b.label)
    })
}

/** Which part of a project a path belongs to, for grouping the sources view. */
export type SourceArea = "raw" | "vault" | "repository"

export function sourceArea(path: string): SourceArea {
  if (path.startsWith("raw/")) return "raw"
  if (path.startsWith("wiki/")) return "vault"
  return "repository"
}

export const SOURCE_AREA_LABELS: Record<SourceArea, string> = {
  raw: "Raw sources",
  vault: "Vault pages",
  repository: "Repository files",
}

export interface SourceCoverage {
  /** One record per (scan, path): a file can be read by more than one scan. */
  records: number
  scanned: number
  unscanned: number
  stale: number
  anchors: number
  nodes: number
  /** True only when every record was scanned and nothing has changed since. */
  complete: boolean
  unscannedPaths: string[]
  stalePaths: string[]
}

/** How a scan is named in the interface. */
export function sourceScopeLabel(scope: string): string {
  switch (scope) {
    case "vault":
      return "Vault scan"
    case "repository":
      return "Repository scan"
    default:
      return scope
  }
}

/**
 * Records in reading order: by path, then by the scan that read them, so the
 * aspects of one file sit together without their facts being merged. A record
 * the engine says was skipped stays skipped even when another scan read the
 * same path.
 */
export function orderSourceRecords(sources: readonly EngineSource[]): EngineSource[] {
  return [...sources].sort((a, b) => {
    if (a.path !== b.path) return a.path.localeCompare(b.path)
    return a.scope.localeCompare(b.scope)
  })
}

/**
 * Coverage counted per record, never per path. Counting paths would let a file
 * that one scan skipped look covered because another scan read it.
 */
export function sourceCoverage(sources: readonly EngineSource[]): SourceCoverage {
  const unscannedPaths = sources.filter((source) => !source.scanned).map((source) => source.path)
  const stalePaths = sources.filter((source) => source.stale).map((source) => source.path)
  const scanned = sources.filter((source) => source.scanned).length
  return {
    records: sources.length,
    scanned,
    unscanned: unscannedPaths.length,
    stale: stalePaths.length,
    anchors: sources.reduce((sum, source) => sum + source.anchors, 0),
    nodes: sources.reduce((sum, source) => sum + source.nodes, 0),
    complete: sources.length > 0 && unscannedPaths.length === 0 && stalePaths.length === 0,
    unscannedPaths,
    stalePaths,
  }
}

/** A one-line statement of what the last index pass actually covered. */
export function describeIndex(report: EngineIndexReport | null): string | null {
  if (!report) return null
  const base = `Indexed ${report.vaultPages} vault page${report.vaultPages === 1 ? "" : "s"} and ${report.repositoryFiles} repository file${report.repositoryFiles === 1 ? "" : "s"} (${report.symbols} symbols, ${report.edges} edges) in ${report.durationMs} ms.`
  if (!report.incomplete) return base
  return `${base} The pass was incomplete: ${report.skipped.length} path${report.skipped.length === 1 ? "" : "s"} skipped, so these numbers are a floor.`
}

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB"]

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—"
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024
    unit += 1
  }
  const rounded = unit === 0 ? Math.round(value) : value.toFixed(1)
  return `${rounded} ${BYTE_UNITS[unit]}`
}

export function shortHash(hash: string, length = 12): string {
  if (!hash) return "—"
  return hash.length > length ? `${hash.slice(0, length)}…` : hash
}

/** 0..1 as a percentage, or an honest "not stated". */
export function formatConfidence(confidence: number | null): string {
  if (confidence === null || !Number.isFinite(confidence)) return "not stated by the engine"
  const clamped = Math.min(1, Math.max(0, confidence))
  return `${Math.round(clamped * 100)}%`
}

/** Absolute time, in the reader's own locale and time zone. */
export function formatTimestamp(iso: string | null): string {
  if (!iso) return "never"
  const parsed = new Date(iso)
  if (Number.isNaN(parsed.getTime())) return iso
  return parsed.toLocaleString()
}

/** How long ago, in words the engine's own numbers support. */
export function formatAge(iso: string | null, now = Date.now()): string {
  if (!iso) return "never"
  const parsed = new Date(iso).getTime()
  if (Number.isNaN(parsed)) return iso
  const seconds = Math.max(0, Math.round((now - parsed) / 1000))
  if (seconds < 60) return "just now"
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `${hours} h ago`
  const days = Math.round(hours / 24)
  return `${days} d ago`
}

/** "src/auth/oauth.ts:42-58", or the file alone when the anchor is a whole file. */
export function anchorLabel(evidence: EngineEvidence): string {
  const anchor = evidence.anchor
  if (!anchor) return evidence.kind || "no anchor"
  const parts = [anchor.file]
  if (anchor.symbol) parts.push(`#${anchor.symbol}`)
  if (anchor.lineStart !== undefined && anchor.lineEnd !== undefined) {
    parts.push(`:${anchor.lineStart}-${anchor.lineEnd}`)
  } else if (anchor.lineStart !== undefined) {
    parts.push(`:${anchor.lineStart}`)
  }
  return parts.join("")
}

/** Where a node came from, in one line: attribution as the engine recorded it. */
export function provenanceLabel(node: Pick<EngineNode, "provenance">): string {
  const provenance = node.provenance
  if (!provenance || !provenance.source) return "provenance not recorded"
  const parts = [provenance.source]
  if (provenance.detail) parts.push(provenance.detail)
  if (provenance.session) parts.push(`session ${provenance.session}`)
  return parts.join(" · ")
}

/** A node's status and authority, as the engine spells them. */
export function standingLabel(node: Pick<EngineNode, "authority" | "status">): string {
  return `${node.authority} · ${node.status}`
}
