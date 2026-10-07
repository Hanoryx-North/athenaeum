import { useCallback, useRef, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { installUpdate, type InstallProgress } from "@/lib/self-update"
import { toLatestReleaseUrl } from "@/lib/update-check"

/**
 * Drives "Update and restart" for the banner and the About screen.
 *
 * Tries the signed in-place install first. When that is not possible — no
 * signed bundle for this platform, an install kind that cannot update itself,
 * or any error — it opens the release page instead, so the button is never a
 * dead end, and reports that it did so via `fellBack`.
 */
export function useSelfUpdate(releaseUrl: string) {
  const [progress, setProgress] = useState<InstallProgress | null>(null)
  const [fellBack, setFellBack] = useState(false)
  // A ref, not state: a second click in the same tick must not start a second
  // download before React has re-rendered with `busy`.
  const running = useRef(false)

  const install = useCallback(async () => {
    if (running.current) return
    running.current = true
    setFellBack(false)
    setProgress({ phase: "downloading", downloaded: 0, total: null })
    try {
      const outcome = await installUpdate(setProgress)
      if (outcome.kind === "restarting") return
      if (outcome.kind === "failed") console.error("[self-update] install failed:", outcome.message)
      setFellBack(true)
      try {
        await openUrl(toLatestReleaseUrl(releaseUrl))
      } catch (err) {
        console.error("[self-update] openUrl failed:", err)
      }
    } finally {
      running.current = false
      setProgress((p) => (p?.phase === "restarting" ? p : null))
    }
  }, [releaseUrl])

  return { install, progress, busy: progress !== null, fellBack }
}

/** 0–100 when the total is known, otherwise null. */
export function progressPercent(p: InstallProgress | null): number | null {
  if (!p || p.phase !== "downloading" || !p.total) return null
  return Math.min(100, Math.round((p.downloaded / p.total) * 100))
}
