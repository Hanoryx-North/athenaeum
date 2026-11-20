import { useTranslation } from "react-i18next"
import { PanelLeftClose } from "lucide-react"
import { FileTree } from "./file-tree"

interface SidebarPanelProps {
  onCollapse?: () => void
}

export function SidebarPanel({ onCollapse }: SidebarPanelProps) {
  const { t } = useTranslation()
  return (
    <div className="flex h-full flex-col">
      <div className="flex min-h-8 shrink-0 items-stretch border-b">
        <h2 className="flex flex-1 items-center px-3 text-xs font-medium">{t("sidebar.files")}</h2>
        {onCollapse && (
          <button
            type="button"
            onClick={onCollapse}
            className="flex w-9 shrink-0 items-center justify-center border-l text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            title={t("layout.hideSidebar", "Hide sidebar")}
            aria-label={t("layout.hideSidebar", "Hide sidebar")}
          >
            <PanelLeftClose className="h-4 w-4" />
          </button>
        )}
      </div>
      <div className="flex-1 overflow-hidden">
        <FileTree />
      </div>
    </div>
  )
}
