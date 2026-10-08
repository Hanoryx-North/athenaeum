// Pure helpers behind the release workflow. Everything here is deterministic and
// touches no files or network, so the parts that decide what ships can be
// tested without a CI run. `release.mjs` is the thin CLI around them.

import { createHash, createPublicKey, verify } from "node:crypto"

/** The PDFium build the installers bundle. Pinned, and checked by SHA-256. */
export const PDFIUM_TAG = "chromium/8086"

/**
 * Per Rust target: which PDFium archive to bundle, where the library sits in
 * it, and what the app's lookup (`pdfium_candidate_paths` in
 * src-tauri/src/commands/fs.rs) expects to find it called.
 */
export const PDFIUM_BY_TARGET = {
  "x86_64-pc-windows-msvc": {
    asset: "pdfium-win-x64.tgz",
    sha256: "1fd8af952832dbb0eb16d9249f68fe09e5f5ebf7c3dd9f6066ea2720cc28487d",
    inArchive: "bin/pdfium.dll",
    file: "pdfium.dll",
  },
  "aarch64-apple-darwin": {
    asset: "pdfium-mac-arm64.tgz",
    sha256: "e98679e052c07edbb5a627980902abb823d4b3f35744d877bd21668bd9fc13ab",
    inArchive: "lib/libpdfium.dylib",
    file: "libpdfium.dylib",
  },
  "x86_64-apple-darwin": {
    asset: "pdfium-mac-x64.tgz",
    sha256: "933a85a138f6027243c56bff8676375c33ceeb767401389415ffc44d689ca85d",
    inArchive: "lib/libpdfium.dylib",
    file: "libpdfium.dylib",
  },
  "x86_64-unknown-linux-gnu": {
    asset: "pdfium-linux-x64.tgz",
    sha256: "588577cf52dabc1a444988bac841920df54cc2f141801424de97ab04f4fbb935",
    inArchive: "lib/libpdfium.so",
    file: "libpdfium.so",
  },
  "aarch64-unknown-linux-gnu": {
    asset: "pdfium-linux-arm64.tgz",
    sha256: "e7e2fe4686925618330103cb167950aca5a84bb00fd977a41b86be59dd1480a2",
    inArchive: "lib/libpdfium.so",
    file: "libpdfium.so",
  },
}

export function pdfiumUrl(entry) {
  return `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(PDFIUM_TAG)}/${entry.asset}`
}

/**
 * `MAJOR.MINOR` come from the version checked in; the patch is the number of
 * commits on main, so every push is a strictly newer version and re-running a
 * build reproduces the same one. Bump MAJOR.MINOR by hand for a bigger release.
 *
 * The patch has to stay at or under 65535: it also becomes the MSI product
 * version, whose fields are 16-bit.
 */
export function computeVersion(baseVersion, commitCount) {
  const m = /^(\d+)\.(\d+)\.\d+(?:[-+].*)?$/.exec(baseVersion)
  if (!m) throw new Error(`Base version "${baseVersion}" is not MAJOR.MINOR.PATCH`)
  if (!Number.isInteger(commitCount) || commitCount < 1) {
    throw new Error(`Commit count must be a positive integer, got ${commitCount}`)
  }
  if (commitCount > 65535) {
    throw new Error(
      `Commit count ${commitCount} no longer fits the MSI version field (max 65535). Bump MINOR in package.json.`,
    )
  }
  return `${m[1]}.${m[2]}.${commitCount}`
}

/** Rewrite the top-level "version" in package.json text. */
export function stampPackageJson(text, version) {
  const out = text.replace(/^(\s*"version":\s*)"[^"]*"/m, `$1"${version}"`)
  if (out === text && !text.includes(`"version": "${version}"`)) {
    throw new Error("package.json has no top-level version to stamp")
  }
  return out
}

/** Rewrite `version` under `[workspace.package]` in the root Cargo.toml text. */
export function stampWorkspaceCargo(text, version) {
  const section = /(\[workspace\.package\][^[]*?\nversion\s*=\s*)"[^"]*"/
  if (!section.test(text)) throw new Error("Cargo.toml has no [workspace.package] version to stamp")
  return text.replace(section, `$1"${version}"`)
}

/** A minisign public key as `tauri signer generate` writes it, base64-wrapped. */
export function isValidUpdaterPubkey(value) {
  if (typeof value !== "string" || value.length < 40) return false
  try {
    return Buffer.from(value, "base64").toString("utf8").startsWith("untrusted comment:")
  } catch {
    return false
  }
}

/**
 * The config overlay passed to `tauri build --config`. The base
 * tauri.conf.json stays buildable by anyone with no PDFium download and no
 * signing key; everything that only a release needs lives here.
 */
export function buildReleaseConfig({ version, target, updater, pubkey }) {
  const pdfium = PDFIUM_BY_TARGET[target]
  if (!pdfium) throw new Error(`No PDFium build is pinned for target "${target}"`)

  // Only what people install. The base config says "all" so a developer can
  // build anything locally; a release skips the MSI (the NSIS wizard is the
  // Windows installer) and the RPM, and builds the .app that the macOS updater
  // archive comes from alongside the DMG.
  const bundle = {
    targets: target.includes("windows") ? ["nsis"] : target.includes("apple") ? ["app", "dmg"] : ["appimage", "deb"],
  }
  if (updater) bundle.createUpdaterArtifacts = true

  const bundled = `pdfium/${pdfium.file}`
  if (target.includes("apple")) {
    // Lands in Contents/Frameworks, where the app looks first on macOS.
    bundle.macOS = { frameworks: [bundled] }
  } else {
    bundle.resources = { [bundled]: bundled }
  }

  const config = { version, bundle }
  if (pubkey) config.plugins = { updater: { pubkey } }
  return config
}

/**
 * Where each updater platform key gets its bundle from. The patterns match the
 * names tauri-action gives the uploaded assets. Every platform is required: a
 * latest.json missing one would silently strand its users on an old version.
 */
export const UPDATER_PLATFORMS = {
  "windows-x86_64": /_x64-setup\.exe$/,
  "darwin-aarch64": /_aarch64\.app\.tar\.gz$/,
  "darwin-x86_64": /_x64\.app\.tar\.gz$/,
  "linux-x86_64": /_amd64\.AppImage$/,
  "linux-aarch64": /_aarch64\.AppImage$/,
}

/**
 * @param signatures map of asset name -> contents of its `.sig` file
 */
export function buildLatestJson({ version, notes, pubDate, repo, tag, signatures }) {
  const platforms = {}
  const missing = []
  for (const [key, pattern] of Object.entries(UPDATER_PLATFORMS)) {
    const names = Object.keys(signatures).filter((n) => pattern.test(n))
    if (names.length !== 1) {
      missing.push(`${key} (${names.length} matches for ${pattern})`)
      continue
    }
    platforms[key] = {
      signature: signatures[names[0]].trim(),
      url: `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(names[0])}`,
    }
  }
  if (missing.length > 0) {
    throw new Error(`Cannot build latest.json, no unique signed bundle for: ${missing.join("; ")}`)
  }
  return { version, notes, pub_date: pubDate, platforms }
}

// DER prefix of an Ed25519 SubjectPublicKeyInfo; the 32-byte key follows it.
const ED25519_SPKI_PREFIX = Buffer.from("302a300506032b6570032100", "hex")

/**
 * Check a Tauri updater signature the way the installed app will: the minisign
 * signature must verify, under the public key baked into the app, over the
 * bytes of the bundle. Catching a mismatch here means a wrong signing secret
 * fails the release build instead of shipping a release no installed copy will
 * accept an update from.
 *
 * @param pubkey  the `pubkey` value from tauri.conf.json (base64 of the .pub file)
 * @param sig     contents of the bundle's `.sig` file (base64 of the .minisig text)
 * @param data    the bundle bytes
 */
export function verifyUpdaterSignature({ pubkey, sig, data }) {
  const lines = (b64) => Buffer.from(b64, "base64").toString("utf8").split("\n")
  const pub = Buffer.from(lines(pubkey)[1] ?? "", "base64")
  const signature = Buffer.from(lines(sig.trim())[1] ?? "", "base64")
  if (pub.length !== 42 || signature.length !== 74) return false

  const [pubKeyId, key] = [pub.subarray(2, 10), pub.subarray(10)]
  const [algorithm, sigKeyId, raw] = [signature.subarray(0, 2).toString("latin1"), signature.subarray(2, 10), signature.subarray(10)]
  if (!pubKeyId.equals(sigKeyId)) return false

  // "ED" signs the BLAKE2b-512 digest of the file (what Tauri produces); "Ed"
  // signs the file itself.
  let message
  if (algorithm === "ED") message = createHash("blake2b512").update(data).digest()
  else if (algorithm === "Ed") message = data
  else return false

  const publicKey = createPublicKey({ key: Buffer.concat([ED25519_SPKI_PREFIX, key]), format: "der", type: "spki" })
  return verify(null, message, publicKey, raw)
}

/**
 * Every platform a release builds. `thinLto` marks a runner that cannot finish the app
 * crate's fat-LTO link: the Linux arm64 build died twice, at the same step, about 35
 * minutes in, with "the runner has received a shutdown signal".
 */
export const BUILDS = [
  { label: 'Windows (x64)', os: 'windows-latest', target: 'x86_64-pc-windows-msvc', arch: 'x64', thin_lto: false },
  { label: 'macOS (Apple silicon)', os: 'macos-latest', target: 'aarch64-apple-darwin', arch: 'aarch64', thin_lto: false },
  { label: 'macOS (Intel)', os: 'macos-latest', target: 'x86_64-apple-darwin', arch: 'x64', thin_lto: false },
  { label: 'Linux (x64)', os: 'ubuntu-22.04', target: 'x86_64-unknown-linux-gnu', arch: 'amd64', thin_lto: false },
  // A native ARM runner: no cross-compiling webkit2gtk.
  { label: 'Linux (arm64)', os: 'ubuntu-22.04-arm', target: 'aarch64-unknown-linux-gnu', arch: 'aarch64', thin_lto: true },
]

/**
 * The GitHub Actions matrix for a run. `only` is a comma-separated list of target triples; empty
 * means every build. A dry run can name one platform instead of paying for all five, which
 * matters in a private repository where macOS minutes bill at ten times the rate.
 */
export function selectBuilds(only = '') {
  const wanted = only.split(',').map(t => t.trim()).filter(Boolean)
  if (wanted.length === 0) return { include: BUILDS }
  const unknown = wanted.filter(t => !BUILDS.some(b => b.target === t))
  if (unknown.length > 0) {
    throw new Error(`Unknown target(s): ${unknown.join(', ')}. Known: ${BUILDS.map(b => b.target).join(', ')}`)
  }
  return { include: BUILDS.filter(b => wanted.includes(b.target)) }
}
