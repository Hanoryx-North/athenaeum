#!/usr/bin/env node
// One-time setup for signed auto-updates. Run it once, on your own machine:
//
//   node scripts/release/setup-updater-key.mjs
//
// It generates the updater signing key pair, writes the PUBLIC key into
// src-tauri/tauri.conf.json (commit that), and stores the PRIVATE key and its
// password as the GitHub secrets the release workflow reads — straight from the
// key file to GitHub, never printed. It needs the GitHub CLI (`gh`), logged in
// with permission to set secrets on the repository.
//
// The private key is the only thing that can sign an update that installed
// copies will accept. Lose it and every copy already in the wild is stranded on
// its current version; leak it and anyone can push code to every user. The key
// file stays outside the repository (default: ~/.athenaeum-release/); back it up.

import { execFileSync, spawnSync } from "node:child_process"
import { randomBytes } from "node:crypto"
import { chmodSync, existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs"
import { homedir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { isValidUpdaterPubkey } from "./lib.mjs"

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const confPath = join(root, "src-tauri", "tauri.conf.json")
const repo = "Hanoryx-North/athenaeum"

const argv = process.argv.slice(2)
const flag = (name) => {
  const i = argv.indexOf(`--${name}`)
  return i === -1 ? undefined : argv[i + 1]
}
const keyDir = flag("key-dir") ?? join(homedir(), ".athenaeum-release")
const keyFile = join(keyDir, "updater.key")
const skipSecrets = argv.includes("--no-secrets")
const passwordFile = join(keyDir, "updater.password")

const conf = JSON.parse(readFileSync(confPath, "utf8"))
if (isValidUpdaterPubkey(conf.plugins?.updater?.pubkey) && !existsSync(keyFile)) {
  console.error(
    "tauri.conf.json already has an updater public key, but there is no private key at\n" +
      `  ${keyFile}\n` +
      "Replacing the public key would strand every installed copy, so this script will not.\n" +
      "If the key lives elsewhere, point --key-dir at it and re-run to just set the secrets.",
  )
  process.exit(1)
}

mkdirSync(keyDir, { recursive: true })

if (!existsSync(keyFile)) {
  const password = randomBytes(24).toString("base64url")
  const tauri = join(root, "node_modules", ".bin", process.platform === "win32" ? "tauri.cmd" : "tauri")
  const r = spawnSync(tauri, ["signer", "generate", "--ci", "-p", password, "-w", keyFile], {
    cwd: root,
    stdio: ["ignore", "inherit", "inherit"],
    shell: process.platform === "win32",
  })
  if (r.status !== 0) {
    console.error("Key generation failed. Run `npm ci` first so the Tauri CLI is installed.")
    process.exit(1)
  }
  writeFileSync(passwordFile, password, { mode: 0o600 })
  // The CLI writes the key world-readable. Owner-only (a no-op on Windows).
  chmodSync(keyFile, 0o600)
  console.log(`Wrote ${keyFile} and ${passwordFile}. BACK THESE UP somewhere private.`)
} else {
  console.log(`Using existing key at ${keyFile}`)
}

const pubkey = readFileSync(`${keyFile}.pub`, "utf8").trim()
if (!isValidUpdaterPubkey(pubkey)) {
  console.error(`${keyFile}.pub does not look like a minisign public key.`)
  process.exit(1)
}
conf.plugins = { ...conf.plugins, updater: { ...conf.plugins?.updater, pubkey } }
writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n")
console.log("Updated src-tauri/tauri.conf.json with the public key. Commit it.")

if (skipSecrets) {
  console.log(
    `\nSet these repository secrets yourself (Settings → Secrets and variables → Actions):\n` +
      `  TAURI_SIGNING_PRIVATE_KEY           contents of ${keyFile}\n` +
      `  TAURI_SIGNING_PRIVATE_KEY_PASSWORD  contents of ${passwordFile}`,
  )
} else {
  try {
    const set = (name, file) =>
      execFileSync("gh", ["secret", "set", name, "--repo", repo], {
        input: readFileSync(file),
        stdio: ["pipe", "inherit", "inherit"],
      })
    set("TAURI_SIGNING_PRIVATE_KEY", keyFile)
    set("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", passwordFile)
    console.log(`Set TAURI_SIGNING_PRIVATE_KEY and TAURI_SIGNING_PRIVATE_KEY_PASSWORD on ${repo}.`)
  } catch {
    console.error(
      "\nCould not set the secrets with `gh` (not installed, not logged in, or no permission).\n" +
        "Re-run with --no-secrets for manual instructions, or `gh auth login` and run this again.",
    )
    process.exit(1)
  }
}

console.log("\nNext: commit src-tauri/tauri.conf.json and push to main. The next push releases.")
