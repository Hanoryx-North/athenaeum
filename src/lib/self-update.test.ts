import { beforeEach, describe, expect, it, vi } from "vitest"
import { formatBytes, installUpdate, type InstallProgress } from "./self-update"

const check = vi.fn()
const relaunch = vi.fn()

vi.mock("@tauri-apps/plugin-updater", () => ({ check: (...a: unknown[]) => check(...a) }))
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: (...a: unknown[]) => relaunch(...a) }))

type Emit = (event: unknown) => void

/** An Update whose download replays the given events through the callback. */
function updateEmitting(events: unknown[]) {
  return {
    downloadAndInstall: vi.fn(async (cb: Emit) => {
      for (const e of events) cb(e)
    }),
  }
}

describe("installUpdate", () => {
  beforeEach(() => {
    check.mockReset()
    relaunch.mockReset()
  })

  it("reports unavailable, without restarting, when the updater finds nothing", async () => {
    check.mockResolvedValue(null)
    await expect(installUpdate()).resolves.toEqual({ kind: "unavailable" })
    expect(relaunch).not.toHaveBeenCalled()
  })

  it("downloads, installs and then restarts, reporting each phase in order", async () => {
    check.mockResolvedValue(
      updateEmitting([
        { event: "Started", data: { contentLength: 300 } },
        { event: "Progress", data: { chunkLength: 100 } },
        { event: "Progress", data: { chunkLength: 200 } },
        { event: "Finished" },
      ]),
    )
    const seen: InstallProgress[] = []

    const outcome = await installUpdate((p) => seen.push(p))

    expect(outcome).toEqual({ kind: "restarting" })
    expect(relaunch).toHaveBeenCalledTimes(1)
    expect(seen.map((p) => p.phase)).toEqual([
      "downloading", // before the first event
      "downloading", // Started
      "downloading", // 100
      "downloading", // 300
      "installing",
      "restarting",
    ])
    expect(seen[3]).toEqual({ phase: "downloading", downloaded: 300, total: 300 })
  })

  it("keeps total null when the server sends no length", async () => {
    check.mockResolvedValue(
      updateEmitting([{ event: "Started", data: {} }, { event: "Progress", data: { chunkLength: 5 } }]),
    )
    const seen: InstallProgress[] = []
    await installUpdate((p) => seen.push(p))
    expect(seen.every((p) => p.total === null)).toBe(true)
  })

  it("returns a failure instead of throwing when the signature or network fails", async () => {
    check.mockResolvedValue({
      downloadAndInstall: vi.fn().mockRejectedValue(new Error("signature verification failed")),
    })
    await expect(installUpdate()).resolves.toEqual({
      kind: "failed",
      message: "signature verification failed",
    })
    expect(relaunch).not.toHaveBeenCalled()
  })

  it("returns a failure when the check itself throws a non-Error", async () => {
    check.mockRejectedValue("no network")
    await expect(installUpdate()).resolves.toEqual({ kind: "failed", message: "no network" })
  })
})

describe("formatBytes", () => {
  it("never shows 0 KB and switches to MB at one mebibyte", () => {
    expect(formatBytes(10)).toBe("1 KB")
    expect(formatBytes(2048)).toBe("2 KB")
    expect(formatBytes(5 * 1024 * 1024)).toBe("5.0 MB")
  })
})
