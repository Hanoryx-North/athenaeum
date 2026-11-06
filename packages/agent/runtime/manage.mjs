#!/usr/bin/env node
// Build and run Athenaeum’s platform launcher and embedded agent modules.
import { spawnSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = dirname(fileURLToPath(import.meta.url))
const source = join(root, 'native-cli')
const [action, ...args] = process.argv.slice(2)
const toolchain = process.env.ATHENAEUM_RUST_TOOLCHAIN || 'stable'
const target = resolve(process.env.ATHENAEUM_NATIVE_TARGET_DIR || join(root, '../../../target/native-runtime'))
const executable = `athenaeum-agent${process.platform === 'win32' ? '.exe' : ''}`
const binary = join(target, args.includes('--release') ? 'release' : 'debug', executable)

function run(command, argv, cwd = source, env = process.env) {
  const result = spawnSync(command, argv, { cwd, env, stdio: 'inherit', windowsHide: true })
  if (result.error) throw result.error
  process.exitCode = result.status ?? 1
  return result.status === 0
}

try {
  if (action === 'build' || action === 'check') {
    run('cargo', [`+${toolchain}`, action, '--locked', '--target-dir', target,
      '-p', 'athenaeum-agent', '--bin', 'athenaeum-agent', ...args])
  } else if (action === 'run') {
    if (!existsSync(binary)) throw new Error('Build the native agent first: node runtime/manage.mjs build')
    run(binary, args, process.cwd())
  } else if (action === 'install') {
    if (args.length !== 1) throw new Error('Usage: node runtime/manage.mjs install <bin-directory>')
    if (!existsSync(binary)) throw new Error('Build the native agent before installing it.')
    const destination = resolve(args[0])
    const installed = join(destination, executable)
    if (existsSync(installed)) throw new Error(`Refusing to overwrite ${installed}; choose another bin directory.`)
    mkdirSync(destination, { recursive: true })
    copyFileSync(binary, installed)
    process.stdout.write(`Installed ${installed}\n`)
  } else {
    process.stdout.write('Usage: node runtime/manage.mjs <build|check|run|install> [arguments]\n')
    if (action && action !== '--help') process.exitCode = 2
  }
} catch (error) {
  process.stderr.write(`${error.message}\n`)
  process.exitCode = 1
}
