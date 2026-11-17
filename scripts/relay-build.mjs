// Build only: Relay installs and launches the finished bundle separately.
import { execFileSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { buildReleaseConfig } from './release/lib.mjs'
import { downloadPdfium } from './release/pdfium.mjs'
import { restoreResourceSymlinks, verifyRelativeSymlinks } from './relay-links.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const hostArch = { arm64: 'aarch64', x64: 'x86_64' }[process.arch]
const targetDir = resolve(root, process.env.CARGO_TARGET_DIR || 'target')
const run = (program, args) => execFileSync(program, args, {
  cwd: root, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: targetDir },
})
const node = args => run(process.execPath, args)

function verifyBundle(app, output) {
  for (const file of [
    'Info.plist', 'MacOS/athenaeum-app', 'Frameworks/libpdfium.dylib',
    'Resources/harness-driver.mjs', 'Resources/native-agent-driver.mjs',
    'Resources/install/native.mjs', 'Resources/install/install.mjs',
    'Resources/tools/athenaeum-cli.tar.gz', 'Resources/tools/athenaeum-integrations.mjs',
    'Resources/tools/node/bin/node', 'Resources/tools/node/LICENSE',
    'Resources/tools/LICENSE-athenaeum', 'Resources/tools/LICENSE-integrations',
    'Resources/tools/agent/athenaeum-agent.tgz', 'Resources/tools/agent/install.mjs',
    'Resources/tools/agent/LICENSE', 'Resources/tools/agent/LICENSE-protocol',
    'Resources/licenses/pdfium/LICENSE', 'Resources/licenses/pdfium/third-party/pdfium.txt',
  ]) {
    if (!existsSync(join(app, 'Contents', file))) throw new Error(`Missing bundle component: ${file}`)
  }
  verifyRelativeSymlinks(output)
  run('codesign', ['--verify', '--deep', '--strict', app])
}

try {
  if (process.platform !== 'darwin' || !hostArch || process.env.RELAY_PLATFORM !== 'macos' || process.env.RELAY_ARCH !== hostArch) {
    throw new Error('Relay packaging requires a matching macOS host and RELAY_PLATFORM/RELAY_ARCH.')
  }
  if (!process.env.RELAY_OUTPUT_DIR || !process.env.RELAY_PROJECT_DIR || resolve(process.env.RELAY_PROJECT_DIR) !== root) {
    throw new Error('Set RELAY_PROJECT_DIR and RELAY_OUTPUT_DIR using the Relay build environment.')
  }
  const output = resolve(process.env.RELAY_OUTPUT_DIR)
  const target = `${hostArch}-apple-darwin`
  const work = join(targetDir, 'relay-staging')
  const agent = join(work, 'agent')
  rmSync(work, { recursive: true, force: true })
  mkdirSync(work, { recursive: true })

  node(['packages/integrations/scripts/bundle.mjs'])
  node(['scripts/release/package-agent.mjs', 'packages/agent', 'packages/protocol', agent])
  run('cargo', ['build', '--locked', '-p', 'athenaeum-cli', '--target', target])
  run('cargo', ['build', '--locked', '--manifest-path', 'packages/agent/runtime/native-cli/Cargo.toml',
    '--target-dir', join(targetDir, 'native-runtime'), '--target', target])
  node(['scripts/release/package-tools.mjs', target,
    'packages/integrations/dist/athenaeum-integrations.mjs', agent, 'debug'])
  await downloadPdfium(target, join(root, 'src-tauri', 'pdfium'))

  const version = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')).version
  const config = buildReleaseConfig({ version, target, updater: false, tools: true })
  config.bundle.targets = ['app']
  config.bundle.createUpdaterArtifacts = false
  config.bundle.macOS.signingIdentity = '-'
  // The pinned PDFium dylib's LC_BUILD_VERSION requires macOS 13.
  config.bundle.macOS.minimumSystemVersion = '13.0'
  config.bundle.resources = {
    ...config.bundle.resources,
    'pdfium/LICENSE': 'licenses/pdfium/LICENSE',
    'pdfium/licenses/': 'licenses/pdfium/third-party/',
  }
  writeFileSync(join(root, 'src-tauri', 'tauri.relay.conf.json'), JSON.stringify(config, null, 2) + '\n')
  node(['node_modules/@tauri-apps/cli/tauri.js', 'build', '--debug', '--target', target,
    '--bundles', 'app', '--config', 'src-tauri/tauri.relay.conf.json', '--', '--locked'])

  const app = join(targetDir, target, 'debug', 'bundle', 'macos', 'Athenaeum.app')
  if (!existsSync(join(app, 'Contents', 'MacOS', 'athenaeum-app'))) {
    throw new Error(`Desktop bundle is missing: ${app}`)
  }
  mkdirSync(output, { recursive: true })
  const installed = join(output, 'Athenaeum.app')
  cpSync(app, installed, { recursive: true, verbatimSymlinks: true })
  restoreResourceSymlinks(join(root, 'src-tauri', 'tools'), join(installed, 'Contents', 'Resources', 'tools'))
  run('codesign', ['--force', '--deep', '--sign', '-', installed])
  verifyBundle(installed, output)
  console.log(`Relay app ready: ${installed}`)
} catch (error) {
  console.error(`Relay build failed: ${error.message}`)
  process.exitCode = 1
}
