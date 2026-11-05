// Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
// Modified for Athenaeum: the crate version is inherited from the workspace,
// so the workspace manifest is the one to assert against.
import { readFileSync } from "node:fs"
import { describe, expect, it } from "vitest"
import { CHANGELOG } from "@/lib/changelog"

function readJson(path: URL): Record<string, unknown> {
  return JSON.parse(readFileSync(path, "utf8")) as Record<string, unknown>
}

describe("release metadata", () => {
  it("keeps app manifests and the latest changelog version aligned", () => {
    const packageJson = readJson(new URL("../../package.json", import.meta.url))
    const tauriConfig = readJson(new URL("../../src-tauri/tauri.conf.json", import.meta.url))
    const cargoToml = readFileSync(new URL("../../Cargo.toml", import.meta.url), "utf8")
    const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1]

    expect(CHANGELOG[0]?.version).toBe(packageJson.version)
    expect(tauriConfig.version).toBe(packageJson.version)
    expect(cargoVersion).toBe(packageJson.version)
  })
})
