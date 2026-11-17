import { Bot, Cloud, PlugZap } from "lucide-react"
import { useConnectorsStore, type ConnectorPage } from "@/stores/connectors-store"
import { CodingAgentsView } from "./coding-agents-view"
import { ActiveModelStatus } from "./active-model-status"
import { LibraryAccount } from "./library-account"
import { LlmProviderSection } from "@/components/settings/sections/llm-provider-section"
import { cn } from "@/lib/utils"

const PAGES: { id: ConnectorPage; label: string; icon: typeof Bot }[] = [
  { id: "agents", label: "Coding agent", icon: PlugZap },
  { id: "account", label: "Library account", icon: Cloud },
  { id: "model", label: "AI model", icon: Bot },
]

export function ConnectorsView() {
  const { page, setPage } = useConnectorsStore()
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <header className="shrink-0 border-b px-6 py-5">
        <h1 className="text-xl font-semibold">Connectors</h1>
      </header>
      <div className="flex min-h-0 flex-1 flex-col sm:flex-row">
        <nav
          aria-label="Connectors"
          className="flex shrink-0 gap-1 overflow-auto border-b bg-muted/20 p-3 sm:w-48 sm:flex-col sm:border-b-0 sm:border-r"
        >
          {PAGES.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              onClick={() => setPage(id)}
              aria-current={page === id ? "page" : undefined}
              className={cn(
                "flex items-center gap-2 whitespace-nowrap rounded-lg px-3 py-2.5 text-left text-sm",
                page === id
                  ? "bg-accent font-medium text-accent-foreground"
                  : "text-muted-foreground hover:bg-muted",
              )}
            >
              <Icon className="h-4 w-4" />
              {label}
            </button>
          ))}
        </nav>
        <div className="min-h-0 min-w-0 flex-1 overflow-y-auto px-6 py-8">
          <div className="mx-auto max-w-xl">
            {page === "agents" && <CodingAgentsView />}
            {page === "account" && <LibraryAccount />}
            {page === "model" && (
              <div className="space-y-6">
                <header>
                  <h2 className="text-2xl font-semibold">AI model</h2>
                </header>
                <ActiveModelStatus />
                <LlmProviderSection />
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  )
}
