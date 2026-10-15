#!/usr/bin/env node
// Package the MIT adapter and its MIT protocol dependency as an installable
// artifact. A checkout-relative file: dependency cannot ship to a user's PC.
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const [agentRepo, protocolRepo, output] = process.argv.slice(2)
if (!agentRepo || !protocolRepo || !output) throw new Error('Usage: package-agent.mjs AGENT_REPO PROTOCOL_REPO OUTPUT_DIR')
const work = mkdtempSync(join(tmpdir(), 'athenaeum-agent-package-'))
const notice = resolve(dirname(fileURLToPath(import.meta.url)), '../../install/NOTICE-MIT.txt')
const license = repo => existsSync(join(repo, 'LICENSE')) ? join(repo, 'LICENSE') : notice
try {
  const agent = JSON.parse(readFileSync(join(agentRepo, 'package.json'), 'utf8'))
  const protocol = JSON.parse(readFileSync(join(protocolRepo, 'package.json'), 'utf8'))
  for (const name of ['lib', 'cordis.patch.yml', 'profile']) {
    cpSync(join(agentRepo, name), join(work, name), { recursive: true })
  }
  cpSync(license(agentRepo), join(work, 'LICENSE'))
  agent.dependencies['@athenaeum/protocol'] = protocol.version
  agent.bundleDependencies = ['@athenaeum/protocol']
  // Build-only checkout links belong to neither the installed package nor the
  // tarball metadata pnpm resolves while mounting the profile.
  delete agent.devDependencies
  delete agent.packageManager
  agent.files.push('LICENSE')
  writeFileSync(join(work, 'package.json'), JSON.stringify(agent, null, 2) + '\n')
  const bundled = join(work, 'node_modules', '@athenaeum', 'protocol')
  mkdirSync(bundled, { recursive: true })
  for (const name of ['package.json', 'src', 'dist', 'fixtures', 'README.md']) {
    cpSync(join(protocolRepo, name), join(bundled, name), { recursive: true })
  }
  cpSync(license(protocolRepo), join(bundled, 'LICENSE'))
  mkdirSync(output, { recursive: true })
  // Invoke npm's JS entry directly when provided by CI; the command is a build
  // tool only, never something the desktop runs on a user's behalf.
  const result = JSON.parse(execFileSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['pack', '--ignore-scripts', '--json', '--pack-destination', resolve(output)], { cwd: work, encoding: 'utf8' }))[0]
  cpSync(join(output, result.filename), join(output, 'athenaeum-agent.tgz'))
  rmSync(join(output, result.filename))
  cpSync(join(agentRepo, 'install', 'install.mjs'), join(output, 'install.mjs'))
  cpSync(join(agentRepo, 'harness.pin.json'), join(output, 'pins.json'))
  cpSync(license(agentRepo), join(output, 'LICENSE'))
  cpSync(license(protocolRepo), join(output, 'LICENSE-protocol'))
  console.log(`Packaged Athenaeum Agent with protocol ${protocol.version}`)
} finally {
  rmSync(work, { recursive: true, force: true })
}
