import { beforeEach, describe, expect, it, vi } from "vitest"

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  localDataDir: vi.fn(),
  ensureProjectId: vi.fn(),
  upsertProjectInfo: vi.fn(),
}))

vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }))
vi.mock("@tauri-apps/api/path", () => ({ localDataDir: mocks.localDataDir }))
vi.mock("@/lib/project-identity", () => ({
  ensureProjectId: mocks.ensureProjectId,
  upsertProjectInfo: mocks.upsertProjectInfo,
}))

import { createProject, defaultWikiDirectory } from "./fs"

describe("wiki storage defaults", () => {
  beforeEach(() => {
    vi.resetAllMocks()
    mocks.ensureProjectId.mockResolvedValue("wiki-id")
    mocks.upsertProjectInfo.mockResolvedValue(undefined)
  })

  it.each([
    ["/Users/me/Library/Application Support/", "/Users/me/Library/Application Support/Athenaeum/Wikis"],
    ["C:\\Users\\me\\AppData\\Local\\", "C:/Users/me/AppData/Local/Athenaeum/Wikis"],
    ["/home/me/.local/share", "/home/me/.local/share/Athenaeum/Wikis"],
    ["/mnt/custom-data", "/mnt/custom-data/Athenaeum/Wikis"],
  ])("uses the platform data directory %s", async (base, expected) => {
    mocks.localDataDir.mockResolvedValue(base)
    await expect(defaultWikiDirectory()).resolves.toBe(expected)
  })

  it("creates outside the checkout when no parent is supplied", async () => {
    mocks.localDataDir.mockResolvedValue("/Users/me/Library/Application Support")
    const parent = "/Users/me/Library/Application Support/Athenaeum/Wikis"
    mocks.invoke.mockResolvedValue({ name: "Research", path: `${parent}/Research` })

    await expect(createProject("Research")).resolves.toEqual({
      id: "wiki-id", name: "Research", path: `${parent}/Research`,
    })
    expect(mocks.invoke).toHaveBeenCalledWith("create_project", { name: "Research", path: parent })
    expect(mocks.upsertProjectInfo).toHaveBeenCalledWith("wiki-id", `${parent}/Research`, "Research")
  })

  it("preserves an explicitly chosen parent", async () => {
    mocks.invoke.mockResolvedValue({ name: "Research", path: "/chosen/Research" })
    await createProject("Research", "/chosen")
    expect(mocks.localDataDir).not.toHaveBeenCalled()
    expect(mocks.invoke).toHaveBeenCalledWith("create_project", { name: "Research", path: "/chosen" })
  })

  it("does not fall back to the checkout when data directory lookup fails", async () => {
    mocks.localDataDir.mockRejectedValue(new Error("Data directory unavailable"))
    await expect(createProject("Research")).rejects.toThrow("Data directory unavailable")
    expect(mocks.invoke).not.toHaveBeenCalled()
  })

  it("rejects relative or empty parents before creating files", async () => {
    for (const parent of ["", "relative/path"]) {
      await expect(createProject("Research", parent)).rejects.toThrow(/absolute path/i)
    }
    mocks.localDataDir.mockResolvedValue("relative-data")
    await expect(createProject("Research")).rejects.toThrow(/absolute path/i)
    expect(mocks.invoke).not.toHaveBeenCalled()
  })

  it("preserves creation failures without registering a new wiki", async () => {
    mocks.localDataDir.mockResolvedValue("/data")
    mocks.invoke.mockRejectedValue(new Error("Directory already exists"))
    await expect(createProject("Research")).rejects.toThrow("Directory already exists")
    expect(mocks.ensureProjectId).not.toHaveBeenCalled()
    expect(mocks.upsertProjectInfo).not.toHaveBeenCalled()
  })
})
