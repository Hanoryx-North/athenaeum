import { useEffect, useState, type ReactNode } from "react"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { Copy, Minus, Square, X } from "lucide-react"
import { useWikiStore } from "@/stores/wiki-store"
import logo from "@/assets/logo.png"

function WindowsTitleBar() {
  const projectName = useWikiStore((s) => s.project?.name)
  const [maximized, setMaximized] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const appWindow = getCurrentWindow()
    let disposed = false
    let stop: (() => void) | undefined
    const update = () => {
      void appWindow.isMaximized().then((value) => {
        if (!disposed) setMaximized(value)
      }).catch(console.warn)
    }
    update()
    void appWindow.onResized(update).then((unlisten) => {
      if (disposed) unlisten()
      else stop = unlisten
    }).catch(console.warn)
    return () => { disposed = true; stop?.() }
  }, [])

  async function control(action: "minimize" | "toggleMaximize" | "close") {
    try {
      await getCurrentWindow()[action]()
      setError(null)
    } catch (cause) {
      console.error(`[window] ${action} failed:`, cause)
      setError("Window control failed. Please try again.")
    }
  }

  return (
    <header className="flex h-9 shrink-0 select-none items-center border-b bg-muted/50 text-foreground" aria-label="Window title bar">
      <div data-tauri-drag-region className="flex h-full min-w-0 flex-1 items-center gap-2 pl-3 text-xs">
        <img src={logo} alt="" draggable={false} className="pointer-events-none h-4 w-4 rounded-sm" />
        <span className="pointer-events-none truncate">Athenaeum{projectName ? ` / ${projectName}` : ""}</span>
        {error && <span role="alert" className="pointer-events-none truncate text-destructive">{error}</span>}
      </div>
      <button type="button" aria-label="Minimize" title="Minimize" className="flex h-full w-12 items-center justify-center hover:bg-accent focus-visible:outline-2 focus-visible:-outline-offset-2" onClick={() => void control("minimize")}><Minus className="h-3.5 w-3.5" /></button>
      <button type="button" aria-label={maximized ? "Restore window" : "Maximize"} title={maximized ? "Restore window" : "Maximize"} className="flex h-full w-12 items-center justify-center hover:bg-accent focus-visible:outline-2 focus-visible:-outline-offset-2" onClick={() => void control("toggleMaximize")}>{maximized ? <Copy className="h-3 w-3" /> : <Square className="h-3 w-3" />}</button>
      <button type="button" aria-label="Close window" title="Close window" className="flex h-full w-12 items-center justify-center hover:bg-red-600 hover:text-white focus-visible:outline-2 focus-visible:-outline-offset-2" onClick={() => void control("close")}><X className="h-4 w-4" /></button>
    </header>
  )
}

export function DesktopShell({ children }: { children: ReactNode }) {
  const customTitleBar = document.documentElement.classList.contains("platform-windows")
  return <div className="flex h-full flex-col overflow-hidden">
    {customTitleBar && <WindowsTitleBar />}
    <div className="min-h-0 flex-1">{children}</div>
  </div>
}
