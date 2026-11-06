// Build from a clean output tree so removed connector modules cannot enter npm artifacts.
import { rmSync } from 'node:fs'
import { createRequire } from 'node:module'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'
const require = createRequire(import.meta.url)
const root = fileURLToPath(new URL('../', import.meta.url))
rmSync(new URL('../dist', import.meta.url), { recursive: true, force: true })
const result = spawnSync(process.execPath, [require.resolve('typescript/bin/tsc'), '-p', 'tsconfig.build.json'], { cwd: root, stdio: 'inherit' })
if (result.error) throw result.error
process.exitCode = result.status ?? 1
