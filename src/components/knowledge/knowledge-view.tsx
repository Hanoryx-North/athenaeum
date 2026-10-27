/**
 * The knowledge surface: what the engine found, what it read, what it would
 * tell an agent, and what it is linked to.
 *
 * Four tabs over one workspace and one engine. The header belongs to all of
 * them: it states the workspace's product state (no vault, never indexed,
 * index behind the vault, protocol mismatch, engine unreachable) with the
 * action that fits, so a panel is never left guessing why it is empty.
 *
 * Review — the one place a person decides a proposal — is its own destination
 * in the sidebar, because it is the only surface that changes the vault.
 */
import { useCallback, useState } from "react"
import { Activity, Cloud, FileText, FolderSearch } from "lucide-react"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useKnowledgeViewStore, type KnowledgeTab } from "@/stores/knowledge-view-store"
import { openProjectFolder } from "@/commands/fs"
import { normalizePath } from "@/lib/path-utils"
import { cn } from "@/lib/utils"
import { EngineStateNotice } from "./engine-state-notice"
import { KnowledgeHealth } from "./knowledge-health"
import { KnowledgeSources } from "./knowledge-sources"
import { KnowledgeBrief } from "./knowledge-brief"
import { LibraryPanel } from "./library-panel"

const TABS: { id: KnowledgeTab; label: string; icon: typeof Activity; description: string }[] = [
  {
    id: "health",
    label: "Health",
    icon: Activity,
    description: "Authority, stale evidence, duplicates, contradictions, superseded and unsupported knowledge",
  },
  {
    id: "sources",
    label: "Sources",
    icon: FolderSearch,
    description: "What the engine scanned, with hashes, anchors and coverage",
  },
  {
    id: "brief",
    label: "Brief",
    icon: FileText,
    description: "A bounded knowledge brief for a task, composed by the engine",
  },
  {
    id: "library",
    label: "Library",
    icon: Cloud,
    description: "What this project is linked to, and what this window can and cannot check",
  },
]

export function KnowledgeView() {
  const tab = useKnowledgeViewStore((s) => s.tab)
  const setTab = useKnowledgeViewStore((s) => s.setTab)
  const [rebuilding, setRebuilding] = useState(false)
  const project = useWikiStore((s) => s.project)
  const state = useEngineStore((s) => s.state)
  const findings = useEngineStore((s) => s.findings)
  const proposals = useEngineStore((s) => s.status?.proposals ?? 0)
  const refresh = useEngineStore((s) => s.refresh)
  const rebuildIndex = useEngineStore((s) => s.rebuildIndex)
  const staleResponseDropped = useEngineStore((s) => s.staleResponseDropped)

  const blocking = findings.filter((finding) => finding.blocking).length

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
      <div
        className="flex flex-wrap items-center gap-1 border-b px-3 py-2"
        role="tablist"
        aria-label="Knowledge"
      >
        {TABS.map((entry) => {
          const selected = tab === entry.id
          const Icon = entry.icon
          return (
            <button
              key={entry.id}
              role="tab"
              id={`knowledge-tab-${entry.id}`}
              aria-selected={selected}
              aria-controls={`knowledge-panel-${entry.id}`}
              title={entry.description}
              onClick={() => setTab(entry.id)}
              className={cn(
                "flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                selected
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-accent-foreground",
              )}
            >
              <Icon className="h-3.5 w-3.5" aria-hidden />
              {entry.label}
              {entry.id === "health" && blocking > 0 && (
                <span
                  className="rounded-full bg-destructive px-1.5 text-[10px] text-destructive-foreground"
                  aria-label={`${blocking} blocking findings`}
                >
                  {blocking > 99 ? "99+" : blocking}
                </span>
              )}
            </button>
          )
        })}
      </div>

      <EngineStateNotice
        state={state}
        onRetry={() => void refresh()}
        onRebuildIndex={() => void handleRebuild()}
        rebuilding={rebuilding}
        onOpenProjectSettings={
          project ? () => void openProjectFolder(normalizePath(project.path)) : undefined
        }
      />


      {staleResponseDropped && (
        <p role="status" className="border-b bg-muted/30 px-4 py-1.5 text-xs text-muted-foreground">
          An answer arrived after the project changed and was discarded, so nothing here describes
          the wrong vault.
        </p>
      )}

      {proposals > 0 && (
        <p role="status" className="border-b bg-primary/5 px-4 py-1.5 text-xs">
          {proposals} proposal{proposals === 1 ? "" : "s"} awaiting a decision in{" "}
          <span className="font-medium">Review → Proposals</span>. Nothing becomes permanent until
          you accept it.
        </p>
      )}

      <div
        role="tabpanel"
        id={`knowledge-panel-${tab}`}
        aria-labelledby={`knowledge-tab-${tab}`}
        className="min-h-0 flex-1"
      >
        {tab === "health" && <KnowledgeHealth />}
        {tab === "sources" && <KnowledgeSources />}
        {tab === "brief" && <KnowledgeBrief />}
        {tab === "library" && <LibraryPanel />}
      </div>
    </div>
  )
}
