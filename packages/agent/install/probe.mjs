#!/usr/bin/env node
// Proves an installed agent boots through the same public SDK client Athenaeum's desktop uses.
//
//   node install/probe.mjs <launcher> [project-directory]
//
// No model is contacted: start() performs the SDK initialize handshake and close() shuts down
// the real runtime, so an incomplete profile or a broken SDK path fails here.

import { dirname, join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const [launcher, project = process.cwd()] = process.argv.slice(2)
if (launcher === undefined) {
  console.error('usage: probe.mjs <launcher> [project-directory]')
  process.exit(2)
}

const cwd = resolve(project)
const prefix = dirname(dirname(resolve(launcher)))
const dshHome = join(prefix, 'dsh-home')
const profile = join(dshHome, 'profiles', 'athenaeum')
const sdkPath = join(profile, 'node_modules', '@deepseek-ai', 'dsh-sdk-client', 'lib', 'index.js')
const dshBin = join(prefix, 'runtime', 'node_modules', '@deepseek-ai', 'dsh', 'lib', 'bin.js')
const { DeepSeekHarness } = await import(pathToFileURL(sdkPath).href)

// `start()` uses the same public SDK client Athenaeum's desktop uses. Initialization loads the
// real profile but does not contact a model, so no credential or network request is needed.
const harness = new DeepSeekHarness({
  profile: 'athenaeum',
  dshHome,
  dshBin,
  cwd,
  processCwd: cwd,
  provider: 'deepseek-official',
  model: 'deepseek-v4-flash',
  env: { ...process.env, DEEPSEEK_API_KEY: 'probe' },
})
try {
  await harness.start()
  await harness.close()
  console.log('✓ DeepSeek Harness SDK initialized the Athenaeum profile and shut it down cleanly')
} catch (error) {
  try { await harness.close() } catch {}
  console.error(`✗ Could not initialize the Athenaeum profile through the Harness SDK: ${error.message}`)
  process.exitCode = 1
}
