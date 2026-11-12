import { useEffect } from "react"
import { useBackgroundMemoryStore } from "@/stores/background-memory-store"

/** Start the runtime at app startup, independent of the selected chat provider. */
export function useBackgroundMemory(workspace: string | undefined, ready: boolean): void {
  useEffect(() => {
    if (!ready || !workspace) return
    const refresh = () => { void useBackgroundMemoryStore.getState().refresh(workspace) }
    refresh()
    const timer = setInterval(refresh, 15000)
    return () => clearInterval(timer)
  }, [workspace, ready])
}
