#!/usr/bin/env node
// Stage architecture-matched CLI, private Node, integration, and agent installer
// resources before Tauri bundles them. Cross-target staging never executes Node.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const tar = process.platform === 'win32' ? join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe') : 'tar'
const [target, integration, agentDir, profile = 'release'] = process.argv.slice(2)
const targetDir = resolve(root, process.env.CARGO_TARGET_DIR || 'target')
const version = 'v22.22.0'
const nodes = {
  'aarch64-apple-darwin': ['darwin-arm64', 'tar.gz', '5ed4db0fcf1eaf84d91ad12462631d73bf4576c1377e192d222e48026a902640'],
  'x86_64-apple-darwin': ['darwin-x64', 'tar.gz', '5ea50c9d6dea3dfa3abb66b2656f7a4e1c8cef23432b558d45fb538c7b5dedce'],
  'x86_64-pc-windows-msvc': ['win-x64', 'zip', 'c97fa376d2becdc8863fcd3ca2dd9a83a9f3468ee7ccf7a6d076ec66a645c77a'],
  'x86_64-unknown-linux-gnu': ['linux-x64', 'tar.xz', '9aa8e9d2298ab68c600bd6fb86a6c13bce11a4eca1ba9b39d79fa021755d7c37'],
  'aarch64-unknown-linux-gnu': ['linux-arm64', 'tar.xz', '1bf1eb9ee63ffc4e5d324c0b9b62cf4a289f44332dfef9607cea1a0d9596ba6f'],
}

try {
  if (!nodes[target] || !integration || !agentDir || !['release', 'debug'].includes(profile)) throw new Error('Usage: package-tools.mjs TARGET INTEGRATION_FILE AGENT_ARTIFACT_DIR [release|debug]')
  const windows = target.includes('windows')
  let cli = join(targetDir, target, profile, windows ? 'athenaeum.exe' : 'athenaeum')
  if (!existsSync(cli)) {
    // A native cargo build omits the target triple directory. Never use that fallback
    // for a cross-target package: the compiler must identify the exact requested host.
    const host = /^host: (.+)$/m.exec(execFileSync('rustc', ['-vV'], { encoding: 'utf8' }))?.[1]?.trim()
    if (host === target) cli = join(targetDir, profile, windows ? 'athenaeum.exe' : 'athenaeum')
  }
  const nativeName = windows ? "athenaeum-agent.exe" : "athenaeum-agent"
  const native = join(targetDir, 'native-runtime', target, profile, nativeName)
  if (!existsSync(native)) throw new Error(`Missing native agent: ${native}`)
  const integrationLicense = [join(dirname(integration), 'LICENSE-integrations'), resolve(dirname(integration), '../LICENSE'), join(root, 'install', 'NOTICE-MIT.txt')].find(existsSync)
  if (!integrationLicense) throw new Error('The MCP integration artifact must include its MIT LICENSE-integrations notice')
  for (const file of [cli, integration, ...['install.mjs', 'pins.json', 'athenaeum-agent.tgz', 'LICENSE'].map(f => join(agentDir, f))]) {
    if (!existsSync(file)) throw new Error(`Missing packaged component: ${file}`)
  }
  const out = join(root, 'src-tauri', 'tools')
  rmSync(out, { recursive: true, force: true })
  mkdirSync(out, { recursive: true })
  cpSync(integration, join(out, 'athenaeum-integrations.mjs'))
  cpSync(integrationLicense, join(out, 'LICENSE-integrations'))
  cpSync(agentDir, join(out, 'agent'), { recursive: true })
  // MIT components stay separate artifacts and retain their own licence.
  cpSync(join(root, 'LICENSE'), join(out, 'LICENSE-athenaeum'))

  const work = mkdtempSync(join(tmpdir(), 'athenaeum-tools-'))
  try {
    const [platform, ext, expected] = nodes[target]
    const name = `node-${version}-${platform}`
    const response = await fetch(`https://nodejs.org/dist/${version}/${name}.${ext}`)
    if (!response.ok) throw new Error(`Node download failed: HTTP ${response.status}`)
    const data = Buffer.from(await response.arrayBuffer())
    if (createHash('sha256').update(data).digest('hex') !== expected) throw new Error('Private Node checksum mismatch')
    const archive = join(work, `node.${ext}`)
    writeFileSync(archive, data)
    execFileSync(tar, ['-xf', archive, '-C', work], { stdio: 'inherit' })
    cpSync(join(work, name), join(out, 'node'), { recursive: true, verbatimSymlinks: true })
    mkdirSync(join(work, 'cli'))
    cpSync(cli, join(work, 'cli', windows ? 'athenaeum.exe' : 'athenaeum'))
    cpSync(cli, join(work, 'cli', windows ? 'wiki.exe' : 'wiki'))
    cpSync(native, join(work, "cli", nativeName))
    cpSync(join(root, 'LICENSE'), join(work, 'cli', 'LICENSE'))
    cpSync(join(root, 'LICENSE'), join(work, 'cli', 'LICENSE-native-agent'))
    if (!windows) {
      chmodSync(join(work, 'cli', 'athenaeum'), 0o755)
      chmodSync(join(work, 'cli', 'wiki'), 0o755)
      chmodSync(join(work, 'cli', nativeName), 0o755)
    }
    const cliArchive = join(out, windows ? 'athenaeum-cli.zip' : 'athenaeum-cli.tar.gz')
    execFileSync(tar, windows ? ['-a', '-cf', cliArchive, '-C', join(work, 'cli'), '.'] : ['-czf', cliArchive, '-C', join(work, 'cli'), '.'], { stdio: 'inherit' })
    writeFileSync(join(out, 'components.json'), JSON.stringify({ schema: 1, target, node: version,
      agent: JSON.parse(readFileSync(join(agentDir, 'pins.json'), 'utf8')),
    }, null, 2) + '\n')
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
  console.log(`Packaged local tools for ${target}`)
} catch (error) {
  console.error(error instanceof Error ? error.message : error)
  process.exit(1)
}
