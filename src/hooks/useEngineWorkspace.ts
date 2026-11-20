/**
 * Keep the engine-backed stores pointed at the open project.
 *
 * Mounted once so background saving and every knowledge surface follow the
 * open project.
 *
 * The vault is the source of truth, so a write anywhere in the app bumps the
 * wiki store's data version. That is the signal to ask the engine what it sees
 * now — debounced, because a save and its file-tree refresh arrive together,
 * and because the person is not waiting on this.
 */
import { useEffect, useRef } from "react"
import { useWikiStore } from "@/stores/wiki-store"
import { useEngineStore } from "@/stores/engine-store"
import { useAutomationStore } from "@/stores/automation-store"
import { useSetupStore } from "@/stores/setup-store"
import { LIBRARY, useLibraryAccountStore } from "@/stores/library-account-store"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"

const REFRESH_DEBOUNCE_MS = 500

export function useEngineWorkspace(): void {
  const projectPath = useWikiStore((s) => s.project?.path ?? null)
  const dataVersion = useWikiStore((s) => s.dataVersion)
  const username = useLibraryAccountStore((s) => s.username)
  // `attach` already reads the workspace once; the first data-version effect
  // would otherwise be a second identical round trip on the same tick.
  const settledProject = useRef<string | null>(null)

  const attach = useEngineStore((s) => s.attach)
  const detach = useEngineStore((s) => s.detach)
  const refresh = useEngineStore((s) => s.refresh)

  useEffect(() => {
    if (!projectPath) return
    let stopped = false
    let running = false
    let lastSync = 0
    const tick = async () => {
      if (stopped || running) return
      running = true
      try {
        await useAutomationStore.getState().run(projectPath)
        if (stopped) return
        const automation = useAutomationStore.getState()
        if (automation.workspace !== projectPath) return
        const engine = useEngineStore.getState()
        if (engine.workspace !== projectPath || engine.libraryState.kind === "loading" || engine.libraryError) return
        if (useSetupStore.getState().busy) return
        if (!engine.library?.linked && !username) return
        if (engine.library?.server === LIBRARY && !username) return
        if (engine.library?.linked && !automation.settings.autoSync) return
        if (Date.now() - lastSync < 60_000) return
        lastSync = Date.now()
        const result = await useSetupStore.getState().operate(projectPath, {
          job: "sync_backup", automatic: true,
          ...(!engine.library?.linked ? { server: LIBRARY } : {}),
        })
        if (stopped) return
        await engine.loadLibrary()
        await useAutomationStore.getState().run(projectPath)
        const output = result?.output as { status?: string; push?: { nothingToPush?: boolean }; pull?: { changed?: boolean } } | null
        if (result?.exitCode === 0 && output?.status !== "paused" && (output?.pull?.changed || output?.push?.nothingToPush === false)) {
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
  }, [projectPath, username])

  useEffect(() => {
    if (!projectPath) {
      detach()
      return
    }
    attach(projectPath)
  }, [projectPath, attach, detach])

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
    }, REFRESH_DEBOUNCE_MS)
    return () => clearTimeout(timer)
  }, [projectPath, dataVersion, refresh])
}
