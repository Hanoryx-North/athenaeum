/**
 * Keep the engine-backed stores pointed at the open project.
 *
 * Mounted once, next to the layout: the sidebar's proposal badge, the review
 * queue and the knowledge surfaces all read the same store, so the workspace
 * is attached here rather than by whichever panel happens to mount first.
 *
 * The vault is the source of truth, so a write anywhere in the app bumps the
 * wiki store's data version. That is the signal to ask the engine what it sees
 * now — debounced, because a save and its file-tree refresh arrive together,
 * and because the person is not waiting on this.
 */
import { useEffect, useRef } from "react"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useProposalStore } from "@/stores/proposal-store"
import { useAutomationStore } from "@/stores/automation-store"
import { useSetupStore } from "@/stores/setup-store"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"

const REFRESH_DEBOUNCE_MS = 500

export function useEngineWorkspace(): void {
  const projectPath = useWikiStore((s) => s.project?.path ?? null)
  const dataVersion = useWikiStore((s) => s.dataVersion)
  // `attach` already reads the workspace once; the first data-version effect
  // would otherwise be a second identical round trip on the same tick.
  const settledProject = useRef<string | null>(null)

  const attach = useEngineStore((s) => s.attach)
  const detach = useEngineStore((s) => s.detach)
  const refresh = useEngineStore((s) => s.refresh)
  const attachProposals = useProposalStore((s) => s.attach)
  const detachProposals = useProposalStore((s) => s.detach)

  useEffect(() => {
    if (!projectPath) return
    let stopped = false
    let running = false
    let lastSync = 0
    const tick = async () => {
      if (stopped || running || document.visibilityState !== "visible") return
      running = true
      try {
        await useAutomationStore.getState().run(projectPath)
        if (stopped) return
        const automation = useAutomationStore.getState()
        if (automation.workspace !== projectPath || !automation.settings.autoSync) return
        const engine = useEngineStore.getState()
        if (!engine.library?.linked || engine.library.conflicts.length || useSetupStore.getState().busy) return
        if (Date.now() - lastSync < 60_000) return
        lastSync = Date.now()
        const result = await useSetupStore.getState().operate(projectPath, { job: "sync" })
        if (stopped) return
        await engine.loadLibrary()
        if (result?.exitCode === 0) {
          await refreshProjectFileTree(projectPath, { bumpDataVersion: true })
        }
      } catch (cause) {
        if (!stopped) console.error("Wiki automation failed:", cause)
      } finally { running = false }
    }
    void tick()
    const timer = setInterval(() => void tick(), 15_000)
    const online = () => { lastSync = 0; void tick() }
    window.addEventListener("online", online)
    return () => { stopped = true; clearInterval(timer); window.removeEventListener("online", online) }
  }, [projectPath])

  useEffect(() => {
    if (!projectPath) {
      detach()
      detachProposals()
      return
    }
    attach(projectPath)
    attachProposals(projectPath)
  }, [projectPath, attach, detach, attachProposals, detachProposals])

  useEffect(() => {
    if (!projectPath) return
    if (settledProject.current !== projectPath) {
      settledProject.current = projectPath
      return
    }
    const timer = setTimeout(() => {
      // A vault write can change the workspace and the source records, but the
      // review sweep is the engine's most expensive read and the surfaces that
      // show findings ask for it themselves.
      void refresh({ findings: false })
      void useProposalStore.getState().load()
    }, REFRESH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [projectPath, dataVersion, refresh])
}
