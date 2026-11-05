// Prepare a ready-to-launch local app; reuse Cargo's incremental debug cache.
// Live development can replace target/debug without changing this snapshot.
import { execFileSync } from 'node:child_process'
import { copyFileSync, cpSync, existsSync, mkdirSync, renameSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const target = resolve(repo, process.env.CARGO_TARGET_DIR || 'target')
const executable = process.platform === 'win32' ? 'athenaeum-app.exe' : 'athenaeum-app'
const snapshot = join(target, 'standalone')
const staging = join(target, `standalone-${process.pid}.tmp`)

try {
  execFileSync(process.execPath, [
    join(repo, 'node_modules', '@tauri-apps', 'cli', 'tauri.js'),
    'build', '--debug', '--no-bundle',
  ], { cwd: repo, stdio: 'inherit', env: { ...process.env, CARGO_TARGET_DIR: target } })

  mkdirSync(staging, { recursive: true })
  copyFileSync(join(target, 'debug', executable), join(staging, executable))
  // These resources are normally copied beside the binary by tauri-build.
  for (const resource of ['harness-driver.mjs', 'native-agent-driver.mjs', 'install', 'pdfium']) {
    const source = join(target, 'debug', resource)
    if (existsSync(source)) cpSync(source, join(staging, resource), { recursive: true })
  }
  mkdirSync(snapshot, { recursive: true })
  for (const resource of ['harness-driver.mjs', 'native-agent-driver.mjs', 'install', 'pdfium']) {
    const source = join(staging, resource)
    if (existsSync(source)) cpSync(source, join(snapshot, resource), { recursive: true })
  }
  // Publish only after the build and resource copies succeed. Failed builds
  // leave the previous executable usable. Windows requires quitting it first.
  renameSync(join(staging, executable), join(snapshot, executable))
  console.log(`Standalone app ready: ${join(snapshot, executable)}`)
} catch (error) {
  console.error(`Could not prepare the standalone app: ${error.message}`)
  if (error.code === 'EPERM' || error.code === 'EACCES') {
    console.error('Quit the running Athenaeum app before refreshing it.')
  }
  process.exitCode = 1
} finally {
  rmSync(staging, { recursive: true, force: true })
}
