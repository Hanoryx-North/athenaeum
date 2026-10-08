/**
 * In-place self-update through Tauri's updater plugin.
 *
 * `update-check.ts` decides *whether* a newer release exists (a GitHub
 * Releases lookup) and drives the banner. This module is what the banner's
 * button runs: ask the updater plugin for the signed bundle that matches this
 * platform, download it, install it and restart.
 *
 * The plugin checks the bundle's minisign signature against the public key
 * baked into `tauri.conf.json` before it installs anything. Every way that can
 * fail — no key configured yet, an install kind that cannot update itself (a
 * `.deb`), no network, the release not yet carrying a `latest.json` — comes
 * back as an outcome rather than a throw, so the caller can fall back to
 * sending the person to the download page.
 */

export type InstallPhase = "downloading" | "installing" | "restarting"

export interface InstallProgress {
  phase: InstallPhase
  /** Bytes received so far. */
  downloaded: number
  /** Total bytes when the server reported a length, otherwise null. */
  total: number | null
}

export type InstallOutcome =
  | { kind: "restarting" }
  /** The updater found nothing to install: no signed bundle for this platform. */
  | { kind: "unavailable" }
  | { kind: "failed"; message: string }

export async function installUpdate(
  onProgress?: (progress: InstallProgress) => void,
): Promise<InstallOutcome> {
  try {
    const { check } = await import("@tauri-apps/plugin-updater")
    const update = await check()
    if (!update) return { kind: "unavailable" }

    let downloaded = 0
    let total: number | null = null
    onProgress?.({ phase: "downloading", downloaded, total })

    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          total = event.data.contentLength ?? null
          onProgress?.({ phase: "downloading", downloaded, total })
          break
        case "Progress":
          downloaded += event.data.chunkLength
          onProgress?.({ phase: "downloading", downloaded, total })
          break
        case "Finished":
          onProgress?.({ phase: "installing", downloaded, total })
          break
      }
    })

    // On Windows the installer takes over and ends this process, so the lines
    // below only run on macOS and Linux.
    onProgress?.({ phase: "restarting", downloaded, total })
    const { relaunch } = await import("@tauri-apps/plugin-process")
    await relaunch()
    return { kind: "restarting" }
  } catch (err) {
    return {
      kind: "failed",
      message: err instanceof Error ? err.message : String(err),
    }
  }
}

/** "12.3 MB" style label for a progress readout. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
