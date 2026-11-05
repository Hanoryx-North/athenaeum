#!/usr/bin/env node
// Installs the Athenaeum agent: the DeepSeek Harness runtime, this bundle mounted into a
// profile, and the `athenaeum` command line tool the bundle talks to — all under one prefix,
// with a launcher that runs it. Nothing outside the prefix is touched except a launcher link
// (or, on Windows, an entry on the user's PATH).
//
// Normally run through `install.sh` / `install.ps1`, which first put a private Node next to
// it. Run directly, it needs Node 20 or newer:
//
//   node install/install.mjs [--prefix DIR] [--tarball PATH|URL] [--cli PATH|URL]
//                            [--no-link] [--uninstall]
//
// `--tarball` and `--cli` default to the latest GitHub releases and exist so a build can be
// installed before it is released.

import { execFileSync } from 'node:child_process'
import {
  chmodSync, copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, unlinkSync, writeFileSync,
} from 'node:fs'
import { createWriteStream } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join, resolve, posix, win32 } from 'node:path'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'
import { fileURLToPath, pathToFileURL } from 'node:url'

export const AGENT_RELEASES = 'https://github.com/Hanoryx-North/athenaeum/releases/latest/download'
export const APP_RELEASES = 'https://github.com/Hanoryx-North/athenaeum/releases/latest/download'
export const PROFILE = 'athenaeum'

/** Where everything goes by default, in each platform's convention for per-user program data. */
export function defaultPrefix(platform = process.platform, env = process.env, home = homedir()) {
  const paths = platform === 'win32' ? win32 : posix
  if (platform === 'win32') return paths.join(env.LOCALAPPDATA ?? paths.join(home, 'AppData', 'Local'), 'Athenaeum', 'agent')
  if (platform === 'darwin') return paths.join(home, 'Library', 'Application Support', 'Athenaeum', 'agent')
  return paths.join(env.XDG_DATA_HOME ?? paths.join(home, '.local', 'share'), 'athenaeum', 'agent')
}

/**
 * The Rust target triple Athenaeum publishes its CLI for. Windows on ARM runs the x64 build
 * under emulation, since no native one is published.
 */
export function cliTarget(platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`
  const targets = {
    'win32-x64': 'x86_64-pc-windows-msvc',
    'win32-arm64': 'x86_64-pc-windows-msvc',
    'darwin-arm64': 'aarch64-apple-darwin',
    'darwin-x64': 'x86_64-apple-darwin',
    'linux-x64': 'x86_64-unknown-linux-gnu',
    'linux-arm64': 'aarch64-unknown-linux-gnu',
  }
  const target = targets[key]
  if (!target) throw new Error(`No Athenaeum CLI is published for ${key}`)
  return target
}

export function cliArchiveName(target) {
  return `athenaeum-cli-${target}.${target.includes('windows') ? 'zip' : 'tar.gz'}`
}

/** Absolute paths of everything an install puts under `prefix`. */
export function layout(prefix, platform = process.platform) {
  const win = platform === 'win32'
  const sep = win ? '\\' : '/'
  const p = (...parts) => [prefix, ...parts].join(sep)
  return {
    prefix,
    bin: p('bin'),
    runtime: p('runtime'),
    runtimeBin: p('runtime', 'node_modules', '.bin'),
    dshHome: p('dsh-home'),
    tarball: p('athenaeum-agent.tgz'),
    manifest: p('install.json'),
    launcher: p('bin', win ? 'athenaeum-agent.cmd' : 'athenaeum-agent'),
    cli: p('bin', win ? 'athenaeum.exe' : 'athenaeum'),
    // dsh's JavaScript entry, run directly with Node: no `.cmd` shim, so no shell and no
    // quoting to get wrong when a Windows user name contains a space.
    dshJs: p('runtime', 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js'),
  }
}

/** Single-quote a value for /bin/sh, escaping any single quote inside it. */
export const shQuote = value => `'${String(value).replaceAll("'", "'\\''")}'`

/** The script that runs the agent with everything it needs on PATH, independent of the shell. */
export function launcherScript({ prefix, nodeDir, platform = process.platform }) {
  // `nodeDir` is the directory of the Node that installed this, which is also the one the
  // launcher must keep using: the harness is installed against it.
  const l = layout(prefix, platform)
  if (platform === 'win32') {
    return [
      '@echo off',
      'setlocal',
      `set "DSH_HOME=${l.dshHome}"`,
      `set "PATH=${l.bin};${l.runtimeBin};${nodeDir};%PATH%"`,
      `"${nodeDir}\\node.exe" "${l.dshJs}" --profile ${PROFILE} %*`,
      'exit /b %ERRORLEVEL%',
      '',
    ].join('\r\n')
  }
  return [
    '#!/bin/sh',
    '# Written by the Athenaeum agent installer. Re-run the installer to regenerate it.',
    `DSH_HOME=${shQuote(l.dshHome)}`,
    'export DSH_HOME',
    `PATH=${shQuote(l.bin)}:${shQuote(l.runtimeBin)}:${shQuote(nodeDir)}:"$PATH"`,
    'export PATH',
    `exec ${shQuote(`${nodeDir}/node`)} ${shQuote(l.dshJs)} --profile ${PROFILE} "$@"`,
    '',
  ].join('\n')
}

/**
 * `env` with `dirs` put in front of its PATH. Windows environment keys keep their case
 * (`Path`), so a naive `PATH:` would add a second variable and leave the child free to see
 * either; update whichever key is already there.
 */
export function withPath(env, dirs, platform = process.platform) {
  const key = platform === 'win32' ? (Object.keys(env).find(k => k.toLowerCase() === 'path') ?? 'Path') : 'PATH'
  return { ...env, [key]: [...dirs, env[key]].filter(Boolean).join(platform === 'win32' ? ';' : ':') }
}

function parseArgs(argv) {
  const out = {}
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (!a.startsWith('--')) throw new Error(`Unexpected argument: ${a}`)
    const next = argv[i + 1]
    if (next === undefined || next.startsWith('--')) out[a.slice(2)] = true
    else { out[a.slice(2)] = next; i++ }
  }
  return out
}

const say = message => console.log(`▶ ${message}`)

function run(command, args, options = {}) {
  execFileSync(command, args, { stdio: 'inherit', ...options })
}

/** npm's own entry point, found next to the Node that is running this, so no shell is involved. */
function npmCli() {
  const nodeDir = dirname(process.execPath)
  const candidates = [
    join(nodeDir, 'node_modules', 'npm', 'bin', 'npm-cli.js'),
    join(nodeDir, '..', 'lib', 'node_modules', 'npm', 'bin', 'npm-cli.js'),
  ]
  const found = candidates.find(existsSync)
  if (!found) throw new Error(`Cannot find npm next to ${process.execPath}`)
  return found
}

async function fetchTo(source, destination) {
  mkdirSync(dirname(destination), { recursive: true })
  if (!/^https?:\/\//.test(source)) {
    copyFileSync(resolve(source), destination)
    return
  }
  const response = await fetch(source, { redirect: 'follow' })
  if (!response.ok || response.body === null) {
    throw new Error(
      `Download failed: HTTP ${response.status} for ${source}\n` +
        '  (If this repository is private the file cannot be fetched without signing in; see docs/installing.md.)',
    )
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(destination))
}

/** Windows' own bsdtar reads .zip; a `tar` from Git for Windows on PATH does not. */
function tarCommand() {
  return process.platform === 'win32' ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar'
}

function readPin(here) {
  for (const candidate of [join(here, 'pins.json'), join(here, '..', 'harness.pin.json')]) {
    if (existsSync(candidate)) return JSON.parse(readFileSync(candidate, 'utf8'))
  }
  throw new Error('pins.json not found next to the installer')
}

function addToWindowsPath(dir) {
  const quoted = dir.replaceAll("'", "''")
  const script =
    `$d='${quoted}';$p=[Environment]::GetEnvironmentVariable('Path','User');if(-not $p){$p=''};` +
    "if(($p -split ';') -notcontains $d){[Environment]::SetEnvironmentVariable('Path',(($p.TrimEnd(';')+';'+$d).TrimStart(';')),'User')}"
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script])
}

function linkLauncher(l) {
  const dir = join(homedir(), '.local', 'bin')
  mkdirSync(dir, { recursive: true })
  for (const [name, target] of [['athenaeum-agent', l.launcher], ['athenaeum', l.cli]]) {
    const link = join(dir, name)
    try { unlinkSync(link) } catch { /* nothing there yet */ }
    symlinkSync(target, link)
  }
  const onPath = (process.env.PATH ?? '').split(delimiter).includes(dir)
  if (!onPath) {
    console.log(`\n  ${dir} is not on your PATH. Add it (for example in ~/.profile):\n    export PATH="$HOME/.local/bin:$PATH"`)
  }
}

async function install(args) {
  const here = dirname(fileURLToPath(import.meta.url))
  const pin = readPin(here)
  const platform = process.platform
  const prefix = resolve(typeof args.prefix === 'string' ? args.prefix : defaultPrefix())
  const l = layout(prefix, platform)
  const nodeDir = dirname(process.execPath)
  const tarballSource = typeof args.tarball === 'string' ? args.tarball : `${AGENT_RELEASES}/athenaeum-agent.tgz`
  const target = cliTarget()
  const cliSource = typeof args.cli === 'string' ? args.cli : `${APP_RELEASES}/${cliArchiveName(target)}`

  mkdirSync(l.bin, { recursive: true })
  console.log(`Installing the Athenaeum agent into ${prefix}\n`)

  say('Fetching the agent bundle')
  await fetchTo(tarballSource, l.tarball)

  say(`Installing the DeepSeek Harness runtime ${pin.version}`)
  mkdirSync(l.runtime, { recursive: true })
  writeFileSync(join(l.runtime, 'package.json'), JSON.stringify({ name: 'athenaeum-agent-runtime', private: true }) + '\n')
  // pnpm is what `dsh plugin` shells out to; the harness pins the version it is built against.
  // The published CLI does not depend on the desktop SDK client. Install that
  // explicitly at the same release rather than assuming npm hoists it for us.
  run(process.execPath, [npmCli(), 'install', '--no-audit', '--no-fund', '--loglevel=error', '--prefix', l.runtime,
    `@deepseek-ai/dsh@${pin.version}`, `@deepseek-ai/dsh-sdk-client@${pin.version}`, `pnpm@${pin.pnpm}`])

  say('Fetching the athenaeum command line tool')
  const work = join(prefix, '.download')
  rmSync(work, { recursive: true, force: true })
  mkdirSync(work, { recursive: true })
  const archive = join(work, cliArchiveName(target))
  await fetchTo(cliSource, archive)
  run(tarCommand(), ['-xf', archive, '-C', l.bin])
  rmSync(work, { recursive: true, force: true })
  if (platform !== 'win32') chmodSync(l.cli, 0o755)

  const env = withPath({ ...process.env, DSH_HOME: l.dshHome }, [l.bin, l.runtimeBin, nodeDir])

  say('Mounting the bundle into the harness')
  const profileDir = join(l.dshHome, 'profiles', PROFILE)
  const profilePackage = join(profileDir, 'package.json')
  if (existsSync(profilePackage)) {
    const metadata = JSON.parse(readFileSync(profilePackage, 'utf8'))
    // Older installs mounted the SDK client as a profile plugin. It is a
    // desktop library, not a plugin, and pulls in a duplicate full runtime.
    if (metadata.dependencies?.['@deepseek-ai/dsh-sdk-client']) {
      delete metadata.dependencies['@deepseek-ai/dsh-sdk-client']
      writeFileSync(profilePackage, JSON.stringify(metadata, null, 2) + '\n')
    }
  }
  const dsh = (...args) => run(process.execPath, [l.dshJs, ...args], { env })
  dsh('plugin', '--profile', PROFILE, 'add', l.tarball)
  // The SDK server is what lets Athenaeum's chat panel drive the runtime. The harness ships
  // it, but no bundle mounts it, so the profile adds it and its own patch layer names it.
  // Add the exact copy the runtime shipped with: npm's `latest` tag for this package is a
  // different release line that dsh refuses to load.
  const sdkServer = JSON.parse(readFileSync(join(l.runtime, 'node_modules', '@deepseek-ai', 'dsh-sdk-jsonrpc-server', 'package.json'), 'utf8'))
  dsh('plugin', '--profile', PROFILE, 'add', `@deepseek-ai/dsh-sdk-jsonrpc-server@${sdkServer.version}`)
  // The SDK server imports its shared protocol at runtime. It is a peer dependency,
  // so add the matching package explicitly to the isolated profile as well.
  const sdkProtocol = JSON.parse(readFileSync(join(l.runtime, 'node_modules', '@deepseek-ai', 'dsh-sdk-protocol', 'package.json'), 'utf8'))
  dsh('plugin', '--profile', PROFILE, 'add', `@deepseek-ai/dsh-sdk-protocol@${sdkProtocol.version}`)
  // Athenaeum's desktop launches the supported SDK client from its small local driver.
  // Share the explicitly installed matching client. Mounting it as a profile
  // dependency would install another entire Harness and require interactive
  // native-build approval in pnpm. A directory link keeps Node's normal module
  // resolution in the already installed runtime, without changing the Harness.
  const sdkClient = join(l.runtime, 'node_modules', '@deepseek-ai', 'dsh-sdk-client')
  const sdkLink = join(profileDir, 'node_modules', '@deepseek-ai', 'dsh-sdk-client')
  mkdirSync(dirname(sdkLink), { recursive: true })
  rmSync(sdkLink, { recursive: true, force: true })
  symlinkSync(sdkClient, sdkLink, platform === 'win32' ? 'junction' : 'dir')
  const bundled = join(l.runtime, 'node_modules', 'athenaeum-agent', 'profile', 'cordis.patch.yml')
  const installed = join(l.dshHome, 'profiles', PROFILE, 'node_modules', 'athenaeum-agent', 'profile', 'cordis.patch.yml')
  const patch = [installed, bundled].find(existsSync)
  if (!patch) throw new Error('The installed bundle carries no profile/cordis.patch.yml')
  copyFileSync(patch, join(profileDir, 'cordis.patch.yml'))

  say('Writing the launcher')
  writeFileSync(l.launcher, launcherScript({ prefix, nodeDir, platform }))
  if (platform !== 'win32') chmodSync(l.launcher, 0o755)

  say('Checking the install')
  run(l.cli, ['--version'])
  const config = execFileSync(process.execPath, [l.dshJs, '--profile', PROFILE, '--dump-config'], { env, encoding: 'utf8' })
  for (const id of ['wiki-athenaeum', 'tool-wiki', 'wiki-context', 'sdk-jsonrpc-server']) {
    if (!config.includes(id)) throw new Error(`The composed profile is missing "${id}"; the bundle did not mount.`)
  }

  writeFileSync(l.manifest, JSON.stringify({
    schema: 1,
    harness: pin.version,
    profile: PROFILE,
    launcher: l.launcher,
    node: process.execPath,
    athenaeum: l.cli,
    dshHome: l.dshHome,
    installedAt: new Date().toISOString(),
  }, null, 2) + '\n')

  if (args['no-link'] !== true) {
    if (platform === 'win32') {
      try { addToWindowsPath(l.bin) } catch { console.log(`\n  Add ${l.bin} to your PATH to run "athenaeum-agent" from any terminal.`) }
    } else {
      linkLauncher(l)
    }
  }

  console.log(`\nInstalled. Run it in a project:\n\n  cd your-project\n  athenaeum-agent "why does the login flow validate state first?"\n`)
  console.log(`(Launcher: ${l.launcher})`)
}

function uninstall(args) {
  const prefix = resolve(typeof args.prefix === 'string' ? args.prefix : defaultPrefix())
  const l = layout(prefix)
  // Only ever delete a directory this installer wrote: it leaves a manifest.
  if (!existsSync(l.manifest)) {
    throw new Error(`${prefix} has no install.json, so it was not made by this installer. Nothing removed.`)
  }
  for (const name of ['athenaeum-agent', 'athenaeum']) {
    try { unlinkSync(join(homedir(), '.local', 'bin', name)) } catch { /* not linked */ }
  }
  for (const entry of readdirSync(prefix)) rmSync(join(prefix, entry), { recursive: true, force: true })
  console.log(`Removed ${prefix}`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const args = parseArgs(process.argv.slice(2))
    if (args.uninstall === true) uninstall(args)
    else await install(args)
  } catch (err) {
    console.error(`\nInstall failed: ${err instanceof Error ? err.message : err}`)
    process.exit(1)
  }
}
