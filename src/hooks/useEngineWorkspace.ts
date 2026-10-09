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
