/** Pending knowledge and file review findings. */
import { useState } from "react"
import { useTranslation } from "react-i18next"
import { ClipboardList, FileJson } from "lucide-react"
import { ProposalQueue } from "@/components/review/proposal-queue"
import { LegacyReviewQueue } from "@/components/review/legacy-review-queue"
import { useProposalStore } from "@/stores/proposal-store"
import { useReviewStore } from "@/stores/review-store"
import { cn } from "@/lib/utils"

type ReviewTab = "proposals" | "wiki"

const TABS: { id: ReviewTab; label: string; description: string }[] = [
  {
    id: "proposals",
    label: "Proposals",
    description: "Pages waiting for automatic saving",
  },
  {
    id: "wiki",
    label: "Wiki queue",
    description: "Lint findings and import suggestions",
  },
]

export function ReviewView() {
  const { t } = useTranslation()
  const [tab, setTab] = useState<ReviewTab>("proposals")
  const proposalCount = useProposalStore((s) => s.proposals.length)
  const queueCount = useReviewStore((s) => s.items.filter((item) => !item.resolved).length)

  const counts: Record<ReviewTab, number> = { proposals: proposalCount, wiki: queueCount }

  return (
    <div className="flex h-full flex-col">
      <div
        className="flex flex-wrap items-center gap-1 border-b px-3 py-2"
        role="tablist"
        aria-label={t("review.title")}
      >
        {TABS.map((entry) => {
          const selected = tab === entry.id
          const Icon = entry.id === "proposals" ? ClipboardList : FileJson
          return (
            <button
              key={entry.id}
              role="tab"
              id={`review-tab-${entry.id}`}
              aria-selected={selected}
              aria-controls={`review-panel-${entry.id}`}
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
              {counts[entry.id] > 0 && (
                <span className="rounded-full bg-primary px-1.5 text-[10px] text-primary-foreground">
                  {counts[entry.id] > 99 ? "99+" : counts[entry.id]}
                </span>
              )}
            </button>
          )
        })}
      </div>

      <div
        role="tabpanel"
        id={`review-panel-${tab}`}
        aria-labelledby={`review-tab-${tab}`}
        className="min-h-0 flex-1"
      >
        {tab === "proposals" ? <ProposalQueue /> : <LegacyReviewQueue />}
      </div>
    </div>
  )
}
