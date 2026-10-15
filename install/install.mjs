#!/usr/bin/env node
// Installs Athenaeum for the person's coding agents: the `athenaeum` command, the agent
// integration (the MCP server and hooks Claude Code and Codex use), and the private Node that
// runs it, all under one prefix. Then it connects the agents that are installed. Nothing outside
// the prefix is touched except a few launcher links (or, on Windows, an entry on the user's PATH)
// and the agents' own settings, which `athenaeum-integrations disconnect` undoes exactly.
//
// Normally run through `install.sh` / `install.ps1`, which first put a private Node next to it.
// Run directly it needs Node 20 or newer:
//
//   node install/install.mjs [--prefix DIR] [--cli PATH|URL] [--integrations PATH|URL]
//                            [--agents claude,codex,cursor] [--capture off|instruct|nudge]
//                            [--auto-sync] [--no-agents] [--no-link] [--uninstall]
//
// `--cli` and `--integrations` default to the latest GitHub release and exist so a build can be
// installed before it is released.
//
// Installing is global and does not depend on where it is run. Per project, run
// `athenaeum setup` (or open the project in the Athenaeum app).

import { execFileSync } from 'node:child_process'
import {
  chmodSync, copyFileSync, createWriteStream, existsSync, mkdirSync, readdirSync, rmSync, rmdirSync,
  symlinkSync, unlinkSync, writeFileSync, lstatSync, readlinkSync, readFileSync,
} from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'
import { Readable } from 'node:stream'
import { pipeline } from 'node:stream/promises'
import { pathToFileURL } from 'node:url'

export const RELEASES = 'https://github.com/Hanoryx-North/athenaeum/releases/latest/download'
export const BUNDLE_NAME = 'athenaeum-integrations.mjs'

/** Where everything goes by default, in each platform's convention for per-user program data. */
export function defaultPrefix(platform = process.platform, env = process.env, home = homedir()) {
  if (platform === 'win32') return join(env.LOCALAPPDATA ?? join(home, 'AppData', 'Local'), 'Athenaeum')
  if (platform === 'darwin') return join(home, 'Library', 'Application Support', 'Athenaeum')
  return join(env.XDG_DATA_HOME ?? join(home, '.local', 'share'), 'athenaeum')
}

/** The Rust target triple Athenaeum publishes its CLI for. Windows on ARM runs the x64 build. */
export function cliTarget(platform = process.platform, arch = process.arch) {
  const targets = {
    'win32-x64': 'x86_64-pc-windows-msvc',
    'win32-arm64': 'x86_64-pc-windows-msvc',
    'darwin-arm64': 'aarch64-apple-darwin',
    'darwin-x64': 'x86_64-apple-darwin',
    'linux-x64': 'x86_64-unknown-linux-gnu',
    'linux-arm64': 'aarch64-unknown-linux-gnu',
  }
  const target = targets[`${platform}-${arch}`]
  if (!target) throw new Error(`No Athenaeum CLI is published for ${platform}-${arch}`)
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
  const shim = name => p('bin', win ? `${name}.cmd` : name)
  return {
    prefix,
    bin: p('bin'),
    lib: p('lib'),
    node: p('node'),
    manifest: p('install.json'),
    work: p('.download'),
    cli: p('bin', win ? 'athenaeum.exe' : 'athenaeum'),
    bundle: p('lib', BUNDLE_NAME),
    integrations: shim('athenaeum-integrations'),
    mcp: shim('athenaeum-mcp'),
  }
}

/** The names this installer owns directly under the prefix. The agent installer lives in a sibling directory and is never touched. */
export const OWNED = ['bin', 'lib', 'node', 'install.json', '.download']

export const shQuote = value => `'${String(value).replaceAll("'", "'\\''")}'`

/** A launcher that runs the bundle with the Node that installed it, whatever is on PATH later. */
export function shimScript({ nodeBin, bundle, subcommand = '', platform = process.platform }) {
  if (platform === 'win32') {
    return ['@echo off', `"${nodeBin}" "${bundle}"${subcommand ? ` ${subcommand}` : ''} %*`, 'exit /b %ERRORLEVEL%', ''].join('\r\n')
  }
  return ['#!/bin/sh', '# Written by the Athenaeum installer. Re-run the installer to regenerate it.',
    `exec ${shQuote(nodeBin)} ${shQuote(bundle)}${subcommand ? ` ${subcommand}` : ''} "$@"`, ''].join('\n')
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
const run = (command, args, options = {}) => execFileSync(command, args, { stdio: 'inherit', ...options })

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
        '  (If the repository is private the file cannot be fetched without signing in; download the release assets and pass --cli and --integrations paths. See docs/installing.md.)',
    )
  }
  await pipeline(Readable.fromWeb(response.body), createWriteStream(destination))
}

/** Windows' own bsdtar reads .zip; a `tar` from Git for Windows on PATH does not. */
const tarCommand = () => (process.platform === 'win32' ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar')

function addToWindowsPath(dir) {
  const quoted = dir.replaceAll("'", "''")
  const script =
    `$d='${quoted}';$p=[Environment]::GetEnvironmentVariable('Path','User');if(-not $p){$p=''};` +
    "if(($p -split ';') -notcontains $d){[Environment]::SetEnvironmentVariable('Path',(($p.TrimEnd(';')+';'+$d).TrimStart(';')),'User')}"
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script])
}

function removeFromWindowsPath(dir) {
  const quoted = dir.replaceAll("'", "''")
  const script =
    `$d='${quoted}';$p=[Environment]::GetEnvironmentVariable('Path','User');if($p){` +
    "[Environment]::SetEnvironmentVariable('Path',(($p -split ';' | Where-Object { $_ -and $_ -ne $d }) -join ';'),'User')}"
  run('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script])
}

const LINKS = (l) => [['athenaeum', l.cli], ['athenaeum-integrations', l.integrations], ['athenaeum-mcp', l.mcp]]

function linkLaunchers(l, home) {
  const dir = join(home, '.local', 'bin')
  mkdirSync(dir, { recursive: true })
  for (const [name, target] of LINKS(l)) {
    const link = join(dir, name)
    try { unlinkSync(link) } catch { /* nothing there yet */ }
    symlinkSync(target, link)
  }
  if (!(process.env.PATH ?? '').split(delimiter).includes(dir)) {
    console.log(`\n  ${dir} is not on your PATH. Add it (for example in ~/.profile):\n    export PATH="$HOME/.local/bin:$PATH"`)
  }
}

/** Connect the coding agents with the integration just installed. Never fails the install. */
function connectAgents(l, args, nodeBin) {
  const flags = ['connect', '--athenaeum', l.cli]
  if (typeof args.agents === 'string') flags.push('--agent', args.agents)
  if (typeof args.capture === 'string') flags.push('--capture', args.capture)
  if (args['auto-sync'] === true) flags.push('--sync')
  try {
    run(nodeBin, [l.bundle, ...flags])
    return true
  } catch {
    console.log('\n  The coding agents could not be connected. Run `athenaeum setup` to try again and see why.')
    return false
  }
}

export async function install(args, { env = process.env, home = homedir(), platform = process.platform } = {}) {
  const prefix = resolve(typeof args.prefix === 'string' ? args.prefix : defaultPrefix(platform, env, home))
  const l = layout(prefix, platform)
  const nodeBin = process.execPath
  const base = (typeof args.base === 'string' ? args.base : env.ATHENAEUM_INSTALL_BASE_URL) || RELEASES
  const cliSource = typeof args.cli === 'string' ? args.cli : `${base}/${cliArchiveName(cliTarget(platform))}`
  const bundleSource = typeof args.integrations === 'string' ? args.integrations : `${base}/${BUNDLE_NAME}`

  mkdirSync(l.bin, { recursive: true })
  mkdirSync(l.lib, { recursive: true })
  console.log(`Installing Athenaeum into ${prefix}\n`)

  say('Fetching the athenaeum command line tool')
  rmSync(l.work, { recursive: true, force: true })
  mkdirSync(l.work, { recursive: true })
  const archive = join(l.work, 'athenaeum-cli.archive')
  await fetchTo(cliSource, archive)
  run(tarCommand(), ['-xf', archive, '-C', l.bin])
  rmSync(l.work, { recursive: true, force: true })
  if (platform !== 'win32') chmodSync(l.cli, 0o755)

  say('Fetching the agent integration (MCP server and hooks)')
  await fetchTo(bundleSource, l.bundle)

  say('Writing launchers')
  writeFileSync(l.integrations, shimScript({ nodeBin, bundle: l.bundle, platform }))
  writeFileSync(l.mcp, shimScript({ nodeBin, bundle: l.bundle, subcommand: 'mcp', platform }))
  if (platform !== 'win32') { chmodSync(l.integrations, 0o755); chmodSync(l.mcp, 0o755) }

  say('Checking the install')
  run(l.cli, ['--version'])
  run(nodeBin, [l.bundle, '--help'], { stdio: 'ignore' })

  writeFileSync(l.manifest, JSON.stringify({
    schema: 1,
    prefix,
    athenaeum: l.cli,
    integrations: l.integrations,
    bundle: l.bundle,
    node: nodeBin,
    installedAt: new Date().toISOString(),
  }, null, 2) + '\n')

  if (args['no-link'] !== true) {
    if (platform === 'win32') {
      try { addToWindowsPath(l.bin) } catch { console.log(`\n  Add ${l.bin} to your PATH to run "athenaeum" from any terminal.`) }
    } else {
      linkLaunchers(l, home)
    }
  }

  let connected = false
  if (args['no-agents'] !== true) {
    say('Connecting your coding agents')
    connected = connectAgents(l, args, nodeBin)
  }

  console.log(`\nInstalled.${connected ? ' The coding agents found on this machine are connected; `athenaeum doctor` lists which.' : ''}\n`)
  console.log('Next, in each project you want a knowledge base for:\n')
  console.log('  cd your-project')
  console.log('  athenaeum setup                 # creates the wiki; add --library to back it up online\n')
  console.log('then use Claude Code or Codex as usual. Run `athenaeum doctor` any time to check the wiring.')
  return l
}

export function uninstall(args, { env = process.env, home = homedir(), platform = process.platform } = {}) {
  const prefix = resolve(typeof args.prefix === 'string' ? args.prefix : defaultPrefix(platform, env, home))
  const l = layout(prefix, platform)
  // Only ever delete a directory this installer wrote: it leaves a manifest.
  if (!existsSync(l.manifest)) {
    throw new Error(`${prefix} has no install.json, so it was not made by this installer. Nothing removed.`)
  }
  // Undo the agents' settings first, while the integration still exists to do it exactly.
  const record = JSON.parse(readFileSync(l.manifest, 'utf8'))
  if (platform === 'darwin' && record.shellProfile) {
    // Only remove the exact managed PATH entry, preserving other shell settings.
    const { path, block } = record.shellProfile
    if (typeof path === 'string' && typeof block === 'string' && existsSync(path)) {
      const text = readFileSync(path, 'utf8')
      writeFileSync(path, text.replace(block, ''))
    }
  }
  try { run(process.execPath, [l.bundle, 'disconnect']) } catch { console.log('  (could not remove the agent settings; run `athenaeum-integrations disconnect` if they remain)') }
  const agentName = platform === 'win32' ? 'athenaeum-agent.cmd' : 'athenaeum-agent'
  for (const [name, target] of [...LINKS(l), ['athenaeum-agent', join(l.bin, agentName)]]) {
    const link = join(home, '.local', 'bin', name)
    try {
      // Remove a link only if it is ours: a file the person put there themselves is theirs.
      if (lstatSync(link).isSymbolicLink() && resolve(dirname(link), readlinkSync(link)) === target) unlinkSync(link)
    } catch { /* not linked */ }
  }
  if (platform === 'win32') { try { removeFromWindowsPath(l.bin) } catch { /* best effort */ } }
  for (const entry of OWNED) rmSync(join(prefix, entry), { recursive: true, force: true })
  // The prefix also holds the agent installer's directory when it was installed here; leave that.
  try { if (!readdirSync(prefix).length) rmdirSync(prefix) } catch { /* not empty, or already gone */ }
  console.log(`Removed Athenaeum from ${prefix}. Your projects' wikis and your Library account are untouched.`)
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
