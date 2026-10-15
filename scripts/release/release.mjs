#!/usr/bin/env node
// CLI for the release workflow (.github/workflows/release.yml).
//
//   node scripts/release/release.mjs version
//       Print the version this commit would release as.
//
//   node scripts/release/release.mjs matrix [--only TARGET,TARGET]
//       Print the build matrix as JSON for the workflow (all platforms, or just those named).
//
//   node scripts/release/release.mjs check-signing
//       Fail unless the committed updater public key is real. Run before a
//       publishing build so a release without working auto-update never ships.
//
//   node scripts/release/release.mjs prepare --version V --target TRIPLE
//                                   [--updater] [--pubkey-file FILE]
//       Stamp V into package.json and Cargo.toml, download the pinned PDFium
//       for TRIPLE into src-tauri/pdfium/, and write
//       src-tauri/tauri.release.conf.json for `tauri build --config`.
//
//   node scripts/release/release.mjs verify-signature --file BUNDLE [--pubkey-file FILE]
//       Check BUNDLE.sig against the public key the app ships with (or FILE's).
//       Fails the build when the signing secret does not match the committed key.
//
//   node scripts/release/release.mjs latest-json --version V --tag T --repo O/R
//                                   --sigs DIR --notes FILE --out FILE
//       Build the updater manifest from the downloaded *.sig files.

import { execFileSync } from "node:child_process"
import { createHash } from "node:crypto"
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import {
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

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const path = (...p) => join(root, ...p)

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith("--")) throw new Error(`Unexpected argument: ${a}`)
    const key = a.slice(2)
    const next = argv[i + 1]
    if (next === undefined || next.startsWith("--")) out[key] = true
    else {
      out[key] = next
      i++
    }
  }
  return out
}

function need(args, name) {
  if (typeof args[name] !== "string") throw new Error(`Missing --${name}`)
  return args[name]
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"))
}

function configuredPubkey() {
  return readJson(path("src-tauri", "tauri.conf.json")).plugins?.updater?.pubkey
}

async function downloadPdfium(target) {
  const entry = PDFIUM_BY_TARGET[target]
  if (!entry) throw new Error(`No PDFium build is pinned for target "${target}"`)

  const res = await fetch(pdfiumUrl(entry), { redirect: "follow" })
  if (!res.ok) throw new Error(`PDFium download failed: HTTP ${res.status} for ${pdfiumUrl(entry)}`)
  const bytes = Buffer.from(await res.arrayBuffer())

  const actual = createHash("sha256").update(bytes).digest("hex")
  if (actual !== entry.sha256) {
    throw new Error(`PDFium ${entry.asset} checksum mismatch: expected ${entry.sha256}, got ${actual}`)
  }

  const work = mkdtempSync(join(tmpdir(), "pdfium-"))
  try {
    const archive = join(work, entry.asset)
    writeFileSync(archive, bytes)
    // `tar` ships with Windows 10+ as well as macOS and Linux. Run it from the
    // work directory with relative names: GNU tar reads "C:\..." as host:path.
    execFileSync("tar", ["-xzf", entry.asset, entry.inArchive], { cwd: work, stdio: "inherit" })
    const dest = path("src-tauri", "pdfium")
    rmSync(dest, { recursive: true, force: true })
    mkdirSync(dest, { recursive: true })
    cpSync(join(work, entry.inArchive), join(dest, entry.file))
    console.log(`PDFium ${entry.asset} (${actual.slice(0, 12)}…) -> src-tauri/pdfium/${entry.file}`)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

const commands = {
  version() {
    const base = readJson(path("package.json")).version
    const count = Number(execFileSync("git", ["rev-list", "--count", "HEAD"], { cwd: root, encoding: "utf8" }).trim())
    console.log(computeVersion(base, count))
  },

  matrix(args) {
    console.log(JSON.stringify(selectBuilds(typeof args.only === "string" ? args.only : "")))
  },

  "check-signing"() {
    if (!isValidUpdaterPubkey(configuredPubkey())) {
      console.error(
        [
          "The updater public key in src-tauri/tauri.conf.json is not set.",
          "A release built now would install fine but could never update itself.",
          "Run once, on your own machine:  node scripts/release/setup-updater-key.mjs",
        ].join("\n"),
      )
      process.exit(1)
    }
    console.log("Updater public key is set.")
  },

  async prepare(args) {
    const version = need(args, "version")
    const target = need(args, "target")
    const updater = args.updater === true

    // A dry run builds with a throwaway key, so it passes its public half in;
    // a real release uses the one committed in tauri.conf.json.
    let pubkey
    if (typeof args["pubkey-file"] === "string") pubkey = readFileSync(args["pubkey-file"], "utf8").trim()
    if (updater && !isValidUpdaterPubkey(pubkey ?? configuredPubkey())) {
      throw new Error("--updater needs a real public key (see `check-signing`)")
    }

    writeFileSync(path("package.json"), stampPackageJson(readFileSync(path("package.json"), "utf8"), version))
    writeFileSync(path("Cargo.toml"), stampWorkspaceCargo(readFileSync(path("Cargo.toml"), "utf8"), version))

    await downloadPdfium(target)

    const config = buildReleaseConfig({ version, target, updater, pubkey, tools: true })
    writeFileSync(path("src-tauri", "tauri.release.conf.json"), JSON.stringify(config, null, 2) + "\n")
    console.log(`Prepared ${version} for ${target} (updater artifacts: ${updater})`)
  },

  "verify-signature"(args) {
    const file = need(args, "file")
    const pubkey =
      typeof args["pubkey-file"] === "string" ? readFileSync(args["pubkey-file"], "utf8").trim() : configuredPubkey()
    const ok = verifyUpdaterSignature({
      pubkey,
      sig: readFileSync(`${file}.sig`, "utf8"),
      data: readFileSync(file),
    })
    if (!ok) {
      console.error(
        `${file}.sig does not verify against the updater public key.\n` +
          "The TAURI_SIGNING_PRIVATE_KEY secret does not belong to the public key in src-tauri/tauri.conf.json,\n" +
          "so no installed copy would accept this update. Re-run scripts/release/setup-updater-key.mjs.",
      )
      process.exit(1)
    }
    console.log(`${file}: signature verifies against the app's public key.`)
  },

  "latest-json"(args) {
    const dir = need(args, "sigs")
    const signatures = {}
    for (const f of readdirSync(dir)) {
      if (f.endsWith(".sig")) signatures[f.slice(0, -4)] = readFileSync(join(dir, f), "utf8")
    }
    const manifest = buildLatestJson({
      version: need(args, "version"),
      tag: need(args, "tag"),
      repo: need(args, "repo"),
      notes: readFileSync(need(args, "notes"), "utf8").trim(),
      pubDate: new Date().toISOString(),
      signatures,
    })
    writeFileSync(need(args, "out"), JSON.stringify(manifest, null, 2) + "\n")
    console.log(`Wrote ${args.out} with platforms: ${Object.keys(manifest.platforms).join(", ")}`)
  },
}

const [name, ...rest] = process.argv.slice(2)
if (!commands[name]) {
  console.error(`Usage: release.mjs <${Object.keys(commands).join("|")}> [--options]`)
  process.exit(2)
}
try {
  await commands[name](parseArgs(rest))
} catch (err) {
  console.error(err instanceof Error ? err.message : err)
  process.exit(1)
}
