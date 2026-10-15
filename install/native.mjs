#!/usr/bin/env node
// The desktop's installer entry point. Release builds carry every base component;
// no curl, PowerShell bootstrap, system Node, or release download is needed.
// Agent runtime installation stays in athenaeum_agent's own installer.
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, symlinkSync, lstatSync, readlinkSync, unlinkSync, readFileSync, appendFileSync, realpathSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { homedir } from 'node:os'
import { defaultPrefix, install, layout } from './install.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const tools = resolve(here, '..', 'tools')
const agent = process.argv.includes('--agent')
const noLink = process.argv.includes('--no-link')
const prefix = defaultPrefix()
const l = layout(prefix)
const bundledNode = join(tools, 'node')
const node = join(l.node, process.platform === 'win32' ? 'node.exe' : 'bin/node')
const cliArchive = join(tools, process.platform === 'win32' ? 'athenaeum-cli.zip' : 'athenaeum-cli.tar.gz')
const nodeRelative = process.platform === 'win32' ? 'node.exe' : 'bin/node'

function copyRuntime(destination) {
  const sourceNode = join(bundledNode, nodeRelative)
  const existingNode = join(destination, nodeRelative)
  const hash = file => createHash('sha256').update(readFileSync(file)).digest('hex')
  const sameExecutable = existsSync(existingNode) && hash(sourceNode) === hash(existingNode)
  cpSync(bundledNode, destination, {
    recursive: true, verbatimSymlinks: true,
    // Windows locks an executing node.exe. Keep identical binaries in place
    // while still restoring npm and any other missing runtime resources.
    filter: source => !sameExecutable || resolve(source) !== resolve(sourceNode),
  })
}

try {
  if (process.argv.slice(2).some(arg => !['--agent', '--no-link'].includes(arg))) throw new Error('Unsupported setup option')
  if (!existsSync(cliArchive) || !existsSync(bundledNode)) {
    throw new Error('This build has no packaged command-line tools. Install a native Athenaeum release, then retry setup.')
  }
  // The integration launchers must point at the installed private Node rather
  // than an app resource path that could move with the next desktop update.
  if (!existsSync(node) || realpathSync(process.execPath) !== realpathSync(node)) {
    mkdirSync(prefix, { recursive: true })
    copyRuntime(l.node)
    execFileSync(node, [fileURLToPath(import.meta.url), ...process.argv.slice(2)], { stdio: 'inherit' })
    process.exit(0)
  }

  await install({ cli: cliArchive, integrations: join(tools, 'athenaeum-integrations.mjs'), 'no-agents': true, 'no-link': noLink })

  if (process.platform === 'darwin' && !noLink) {
    // Finder's environment does not include ~/.local/bin. Make the installed
    // commands available in new login terminals without asking for a command.
    const profile = join(homedir(), process.env.SHELL?.endsWith('/bash') ? '.bash_profile' : '.zprofile')
    const marker = '# Athenaeum command-line tools'
    const block = `\n${marker}\nexport PATH="$HOME/.local/bin:$PATH"\n`
    const text = existsSync(profile) ? readFileSync(profile, 'utf8') : ''
    if (!text.includes(marker)) appendFileSync(profile, block)
    const record = JSON.parse(readFileSync(l.manifest, 'utf8'))
    record.shellProfile = { path: profile, block }
    writeFileSync(l.manifest, JSON.stringify(record, null, 2) + '\n')
    console.log('Open a new terminal to use the Athenaeum commands.')
  }

  if (agent) {
    console.log('Installing Athenaeum Agent and its pinned Harness runtime. This step needs internet access.')
    const agentNodeDir = join(prefix, 'agent', 'node')
    copyRuntime(agentNodeDir)
    const agentNode = join(agentNodeDir, process.platform === 'win32' ? 'node.exe' : 'bin/node')
    execFileSync(agentNode, [realpathSync(join(tools, 'agent', 'install.mjs')),
      '--prefix', join(prefix, 'agent'), '--cli', cliArchive,
      '--tarball', join(tools, 'agent', 'athenaeum-agent.tgz'), '--no-link'], { stdio: 'inherit' })
    // Keep the one primary athenaeum command; expose only the agent launcher
    // from its private runtime. Its implementation remains upstream-owned.
    const name = process.platform === 'win32' ? 'athenaeum-agent.cmd' : 'athenaeum-agent'
    const source = join(prefix, 'agent', 'bin', name)
    copyFileSync(source, join(l.bin, name))
    if (process.platform !== 'win32') {
      chmodSync(join(l.bin, name), 0o755)
    }
    if (process.platform !== 'win32' && !noLink) {
      const link = join(homedir(), '.local', 'bin', name)
      if (existsSync(link) || (() => { try { return lstatSync(link).isSymbolicLink() } catch { return false } })()) {
        if (!lstatSync(link).isSymbolicLink() || readlinkSync(link) !== join(l.bin, name)) {
          throw new Error(`${link} already exists. The agent is installed; keep or move that command before retrying.`)
        }
        unlinkSync(link)
      }
      symlinkSync(join(l.bin, name), link)
    }
    console.log('Athenaeum Agent is installed. Run athenaeum-agent in your project, or use the app chat. Configure your model API key before coding.')
  }
} catch (error) {
  if (['EPERM', 'EBUSY', 'ETXTBSY'].includes(error?.code)) {
    console.error('Close running coding agents and terminals using Athenaeum, then retry setup.')
  }
  console.error(`Setup failed: ${error instanceof Error ? error.message : error}`)
  process.exit(1)
}
