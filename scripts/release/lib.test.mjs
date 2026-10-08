import { describe, expect, it } from "vitest"
import {
  BUILDS,
  PDFIUM_BY_TARGET,
  buildLatestJson,
  buildReleaseConfig,
  computeVersion,
  isValidUpdaterPubkey,
  pdfiumUrl,
  selectBuilds,
  stampPackageJson,
  stampWorkspaceCargo,
  verifyUpdaterSignature,
} from "./lib.mjs"

describe("computeVersion", () => {
  it("keeps MAJOR.MINOR and makes the commit count the patch", () => {
    expect(computeVersion("0.1.0", 137)).toBe("0.1.137")
    expect(computeVersion("2.5.9", 1)).toBe("2.5.1")
  })

  it("orders successive pushes as strictly newer versions", () => {
    const [a, b] = [computeVersion("0.1.0", 99), computeVersion("0.1.0", 100)]
    expect(Number(b.split(".")[2])).toBeGreaterThan(Number(a.split(".")[2]))
  })

  it("refuses a patch the MSI version field cannot hold", () => {
    expect(() => computeVersion("0.1.0", 65536)).toThrow(/MSI/)
    expect(computeVersion("0.1.0", 65535)).toBe("0.1.65535")
  })

  it("refuses malformed input rather than shipping a bad version", () => {
    expect(() => computeVersion("0.1", 5)).toThrow()
    expect(() => computeVersion("0.1.0", 0)).toThrow()
    expect(() => computeVersion("0.1.0", Number.NaN)).toThrow()
  })
})

describe("stamping", () => {
  it("changes only the top-level package.json version", () => {
    const text = '{\n  "name": "x",\n  "version": "0.1.0",\n  "dependencies": { "v": "1.0.0" }\n}\n'
    expect(stampPackageJson(text, "0.1.9")).toBe(
      '{\n  "name": "x",\n  "version": "0.1.9",\n  "dependencies": { "v": "1.0.0" }\n}\n',
    )
  })

  it("changes only the workspace package version in Cargo.toml", () => {
    const text = '[workspace]\nmembers = []\n\n[workspace.package]\nversion = "0.1.0"\nedition = "2021"\n\n[workspace.dependencies]\nserde = { version = "1" }\n'
    const out = stampWorkspaceCargo(text, "0.1.9")
    expect(out).toContain('[workspace.package]\nversion = "0.1.9"')
    expect(out).toContain('serde = { version = "1" }')
  })

  it("fails loudly when there is nothing to stamp", () => {
    expect(() => stampWorkspaceCargo("[package]\nname = 'x'\n", "1.0.0")).toThrow()
    expect(() => stampPackageJson("{}", "1.0.0")).toThrow()
  })
})

describe("isValidUpdaterPubkey", () => {
  const real = Buffer.from("untrusted comment: minisign public key: ABCD\nRWQ...\n").toString("base64")

  it("accepts a base64 minisign public key", () => {
    expect(isValidUpdaterPubkey(real)).toBe(true)
  })

  it("rejects the placeholder the repository ships with", () => {
    expect(isValidUpdaterPubkey("UNSET - run scripts/release/setup-updater-key.mjs once")).toBe(false)
  })

  it("rejects empty and non-string values", () => {
    expect(isValidUpdaterPubkey("")).toBe(false)
    expect(isValidUpdaterPubkey(undefined)).toBe(false)
  })
})

describe("PDFium pins", () => {
  it("covers every target the release builds and pins a SHA-256 for each", () => {
    expect(Object.keys(PDFIUM_BY_TARGET).sort()).toEqual([
      "aarch64-apple-darwin",
      "aarch64-unknown-linux-gnu",
      "x86_64-apple-darwin",
      "x86_64-pc-windows-msvc",
      "x86_64-unknown-linux-gnu",
    ])
    for (const entry of Object.values(PDFIUM_BY_TARGET)) {
      expect(entry.sha256).toMatch(/^[0-9a-f]{64}$/)
    }
  })

  it("downloads from the pinned tag, with the slash encoded", () => {
    expect(pdfiumUrl(PDFIUM_BY_TARGET["x86_64-pc-windows-msvc"])).toBe(
      "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F8086/pdfium-win-x64.tgz",
    )
  })
})

describe("buildReleaseConfig", () => {
  it("bundles the Windows DLL as a resource under pdfium/, where the app looks", () => {
    const c = buildReleaseConfig({ version: "0.1.5", target: "x86_64-pc-windows-msvc", updater: false })
    expect(c.version).toBe("0.1.5")
    expect(c.bundle.resources).toEqual({ "pdfium/pdfium.dll": "pdfium/pdfium.dll" })
    expect(c.bundle.createUpdaterArtifacts).toBeUndefined()
    expect(c.plugins).toBeUndefined()
  })

  it("bundles the macOS dylib as a framework, which the app probes first", () => {
    const c = buildReleaseConfig({ version: "0.1.5", target: "aarch64-apple-darwin", updater: false })
    expect(c.bundle.macOS).toEqual({ frameworks: ["pdfium/libpdfium.dylib"] })
    expect(c.bundle.resources).toBeUndefined()
  })

  it("bundles the Linux library as a resource", () => {
    const c = buildReleaseConfig({ version: "0.1.5", target: "x86_64-unknown-linux-gnu", updater: false })
    expect(c.bundle.resources).toEqual({ "pdfium/libpdfium.so": "pdfium/libpdfium.so" })
  })

  it("builds only the installers people use on each platform", () => {
    const targets = (t) => buildReleaseConfig({ version: "1.0.0", target: t, updater: false }).bundle.targets
    expect(targets("x86_64-pc-windows-msvc")).toEqual(["nsis"])
    expect(targets("aarch64-apple-darwin")).toEqual(["app", "dmg"])
    expect(targets("x86_64-unknown-linux-gnu")).toEqual(["appimage", "deb"])
  })

  it("asks for signed updater artifacts and injects the key only when told to", () => {
    const c = buildReleaseConfig({
      version: "0.1.5",
      target: "x86_64-unknown-linux-gnu",
      updater: true,
      pubkey: "KEY",
    })
    expect(c.bundle.createUpdaterArtifacts).toBe(true)
    expect(c.plugins).toEqual({ updater: { pubkey: "KEY" } })
  })

  it("refuses a target it has no PDFium for", () => {
    expect(() => buildReleaseConfig({ version: "1.0.0", target: "riscv64-unknown-linux-gnu", updater: false })).toThrow(/PDFium/)
  })
})

describe("buildLatestJson", () => {
  const signatures = {
    "Athenaeum_0.1.5_x64-setup.exe": "sig-win\n",
    "Athenaeum_aarch64.app.tar.gz": "sig-mac-arm",
    "Athenaeum_x64.app.tar.gz": "sig-mac-x64",
    "Athenaeum_0.1.5_amd64.AppImage": "sig-linux-x64",
    "Athenaeum_0.1.5_aarch64.AppImage": "sig-linux-arm",
  }
  const args = { version: "0.1.5", notes: "n", pubDate: "2026-10-07T00:00:00Z", repo: "o/r", tag: "v0.1.5", signatures }

  it("maps each signed bundle to its updater platform with a trimmed signature and a release URL", () => {
    const json = buildLatestJson(args)
    expect(Object.keys(json.platforms).sort()).toEqual([
      "darwin-aarch64",
      "darwin-x86_64",
      "linux-aarch64",
      "linux-x86_64",
      "windows-x86_64",
    ])
    expect(json.platforms["windows-x86_64"]).toEqual({
      signature: "sig-win",
      url: "https://github.com/o/r/releases/download/v0.1.5/Athenaeum_0.1.5_x64-setup.exe",
    })
    expect(json.version).toBe("0.1.5")
    expect(json.pub_date).toBe("2026-10-07T00:00:00Z")
  })

  it("does not confuse the two Linux or the two macOS architectures", () => {
    const json = buildLatestJson(args)
    expect(json.platforms["linux-x86_64"].signature).toBe("sig-linux-x64")
    expect(json.platforms["linux-aarch64"].signature).toBe("sig-linux-arm")
    expect(json.platforms["darwin-aarch64"].signature).toBe("sig-mac-arm")
    expect(json.platforms["darwin-x86_64"].signature).toBe("sig-mac-x64")
  })

  it("refuses to publish an updater manifest that is missing a platform", () => {
    const { "Athenaeum_aarch64.app.tar.gz": _drop, ...rest } = signatures
    expect(() => buildLatestJson({ ...args, signatures: rest })).toThrow(/darwin-aarch64/)
  })

  it("refuses an ambiguous match rather than picking one", () => {
    expect(() =>
      buildLatestJson({ ...args, signatures: { ...signatures, "Other_x64-setup.exe": "dup" } }),
    ).toThrow(/windows-x86_64/)
  })
})

describe("verifyUpdaterSignature", () => {
  // Produced by `tauri signer sign` with a throwaway key whose private half was
  // discarded: the payload below, signed, and the matching public key.
  const pubkey =
    "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEMwRTdFMzY1NUJDQkQ3MDEKUldRQjE4dGJaZVBud0ROYjE1SHBjelZLdWNmek95U2J5SzlEODY3elpadkFaWG5wSHhEQjhCQ24K"
  const sig =
    "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRQjE4dGJaZVBud0JXc0ZFM09pa2ZkYXVMU293K084TTZEaUR0SmpTRE9uOUdxRTYyb3VaUDRTWWlHbGlPS1dCNHdlUHNPY21YanlEcXM1VlNSSTVUWnBXQml2Z09yNGdrPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxNDEzODU3CWZpbGU6cGF5bG9hZC5iaW4KUVlsM0NNZHNtOVUzQ0tuc1dMdW8wZ1M1TGxzUW9NeE1DNGFGSVdmL2pxZlBaeWE5N3dHRmJVdXdTYlhYWUx0bXlleGVERTROcWdoZk5ubS9zdHArQVE9PQo="
  const payload = Buffer.from("hello athenaeum update payload\n")

  it("accepts the signature a real Tauri signing run produced", () => {
    expect(verifyUpdaterSignature({ pubkey, sig, data: payload })).toBe(true)
  })

  it("rejects a bundle that was changed after signing", () => {
    expect(verifyUpdaterSignature({ pubkey, sig, data: Buffer.from("tampered") })).toBe(false)
  })

  it("rejects a signature made by a different key, which is what a wrong secret looks like", () => {
    // Well-formed and carrying the same key id, so only the cryptographic
    // check can reject it — not a length or id mismatch.
    const real = Buffer.from(Buffer.from(pubkey, "base64").toString("utf8").split("\n")[1], "base64")
    const impostor = Buffer.concat([real.subarray(0, 10), Buffer.alloc(32, 7)])
    const otherPub = Buffer.from(
      `untrusted comment: minisign public key\n${impostor.toString("base64")}\n`,
    ).toString("base64")
    expect(impostor.length).toBe(42)
    expect(verifyUpdaterSignature({ pubkey: otherPub, sig, data: payload })).toBe(false)
  })

  it("rejects garbage instead of throwing", () => {
    expect(verifyUpdaterSignature({ pubkey: "UNSET", sig: "x", data: payload })).toBe(false)
    expect(verifyUpdaterSignature({ pubkey, sig: "", data: payload })).toBe(false)
  })
})

describe('selectBuilds', () => {
  it('builds every platform by default', () => {
    expect(selectBuilds().include).toHaveLength(5)
    expect(selectBuilds('').include).toEqual(BUILDS)
    expect(selectBuilds(' , ').include).toEqual(BUILDS)
  })

  it('builds exactly the platforms a dry run names', () => {
    const picked = selectBuilds('aarch64-unknown-linux-gnu, x86_64-apple-darwin').include.map(b => b.target)
    expect(picked.sort()).toEqual(['aarch64-unknown-linux-gnu', 'x86_64-apple-darwin'])
  })

  it('refuses a typo rather than silently building nothing', () => {
    expect(() => selectBuilds('aarch64-linux')).toThrow(/Unknown target/)
  })

  it('has a PDFium build and a distinct target for every platform', () => {
    for (const b of BUILDS) expect(PDFIUM_BY_TARGET[b.target]).toBeDefined()
    expect(new Set(BUILDS.map(b => b.target)).size).toBe(BUILDS.length)
  })

  it('relaxes link-time optimisation only where the runner could not finish it', () => {
    expect(BUILDS.filter(b => b.thin_lto).map(b => b.target)).toEqual(['aarch64-unknown-linux-gnu'])
  })
})
