import { Menu } from "@base-ui/react/menu"
import {
  Activity, ArrowLeftRight, ClipboardCheck, Ellipsis, FileText, FolderOpen, Globe,
  MessageSquare, Network, NotebookPen, PlugZap, Search, Settings, Sparkles,
} from "lucide-react"
import { useTranslation } from "react-i18next"
import { LibraryAccountMenu } from "@/components/setup/library-account-menu"
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip"
import { useWikiStore, type WikiState } from "@/stores/wiki-store"
import { useResearchStore } from "@/stores/research-store"
import { useUpdateStore, hasAvailableUpdate } from "@/stores/update-store"
import { cn } from "@/lib/utils"
import logo from "@/assets/logo.png"
import { nextResearchPanelNavState } from "./research-panel-nav"

type View = WikiState["activeView"]

const WORKSPACE_VIEWS = [
  { view: "log", icon: NotebookPen, label: "nav.log", fallback: "Log" },
  { view: "wiki", icon: FileText, label: "nav.notes", fallback: "Notes" },
  { view: "search", icon: Search, label: "nav.search" },
  { view: "graph", icon: Network, label: "nav.graph" },
  { view: "sources", icon: FolderOpen, label: "nav.sources" },
  { view: "chat", icon: MessageSquare, label: "nav.chat" },
] satisfies { view: View; icon: typeof FileText; label: string; fallback?: string }[]

const TOOL_VIEWS = [
  { view: "knowledge", icon: Activity, label: "nav.knowledge" },
  { view: "lint", icon: ClipboardCheck, label: "nav.lint" },
  { view: "connectors", icon: PlugZap, label: "nav.connectors", fallback: "Connectors" },
  { view: "skills", icon: Sparkles, label: "nav.skills" },
] satisfies { view: View; icon: typeof FileText; label: string; fallback?: string }[]

const railButton = "relative flex h-10 w-10 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground focus-visible:outline-2 focus-visible:outline-ring"
const menuItem = "flex cursor-default items-center gap-2 rounded-sm px-2.5 py-2 text-sm data-highlighted:bg-accent data-highlighted:text-accent-foreground [&_svg]:h-4 [&_svg]:w-4"

export function IconSidebar({ onSwitchProject, onSignedIn }: {
  onSwitchProject: () => void
  onSignedIn: () => Promise<void>
}) {
  const { t } = useTranslation()
  const activeView = useWikiStore((s) => s.activeView)
  const setActiveView = useWikiStore((s) => s.setActiveView)
  const updateAvailable = useUpdateStore(hasAvailableUpdate)

  function toggleResearch() {
    const state = useResearchStore.getState()
    const next = nextResearchPanelNavState(activeView, state.panelOpen)
    if (next.activeView !== activeView) setActiveView(next.activeView)
    state.setPanelOpen(next.researchPanelOpen)
  }

  return (
    <TooltipProvider delay={300}>
      <nav aria-label="Workspace" className="flex h-full w-12 shrink-0 flex-col items-center gap-1 overflow-y-auto border-r bg-muted/50 py-2">
        <img src={logo} alt="Athenaeum" className="mb-1 h-8 w-8 shrink-0 rounded-[22%]" />
        {WORKSPACE_VIEWS.map(({ view, icon: Icon, label, fallback }) => (
          <Tooltip key={view}>
            <TooltipTrigger aria-label={t(label, { defaultValue: fallback })}
              aria-current={activeView === view ? "page" : undefined}
              onClick={() => setActiveView(view)}
              className={cn(railButton, activeView === view && "bg-accent text-accent-foreground")}>
              <Icon className="h-5 w-5" />
            </TooltipTrigger>
            <TooltipContent side="right">{t(label, { defaultValue: fallback })}</TooltipContent>
          </Tooltip>
        ))}
        <div className="flex-1" />
        <Menu.Root modal={false}>
          <Menu.Trigger className={cn(railButton, TOOL_VIEWS.some(({ view }) => view === activeView) && "bg-accent text-accent-foreground")}
            aria-label="More tools" title="More tools">
            <Ellipsis className="h-5 w-5" />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner side="right" align="end" sideOffset={8} className="z-50">
              <Menu.Popup className="min-w-40 rounded-md border bg-popover p-1 text-popover-foreground shadow-md">
                {TOOL_VIEWS.map(({ view, icon: Icon, label, fallback }) => <Menu.Item key={view} className={menuItem} onClick={() => setActiveView(view)}>
                  <Icon /> {t(label, { defaultValue: fallback })}
                </Menu.Item>)}
                <Menu.Item className={menuItem} onClick={toggleResearch}><Globe /> {t("research.title")}</Menu.Item>
                <Menu.Separator className="my-1 border-t" />
                <Menu.Item className={menuItem} onClick={onSwitchProject}><ArrowLeftRight /> {t("nav.switchProject")}</Menu.Item>
              </Menu.Popup>
            </Menu.Positioner>
          </Menu.Portal>
        </Menu.Root>
        <Tooltip>
          <TooltipTrigger aria-label={t("nav.settings")} aria-current={activeView === "settings" ? "page" : undefined}
            onClick={() => setActiveView("settings")} className={cn(railButton, activeView === "settings" && "bg-accent text-accent-foreground")}>
            <Settings className="h-5 w-5" />
            {updateAvailable && <span className="absolute right-1 top-1 h-2 w-2 rounded-full bg-destructive ring-2 ring-background" title={t("nav.updateAvailable")} />}
          </TooltipTrigger>
          <TooltipContent side="right">{t("nav.settings")}{t("nav.settingsShortcut")}</TooltipContent>
        </Tooltip>
        <LibraryAccountMenu onSignedIn={onSignedIn} />
      </nav>
    </TooltipProvider>
  )
}
