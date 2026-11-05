// Produce the same immutable MIT package for independently deployed clients.
// Run through npm: npm run vendor -- ../athenaeum_library ../athenaeum_api
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const npm = process.env.npm_execpath
if (!npm || process.argv.length < 3) throw new Error('Run npm run vendor -- <client repository> ...')
execFileSync(process.execPath, [join(root, 'scripts/check.mjs')], { cwd: root, stdio: 'inherit' })
const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: root, encoding: 'utf8' }).trim()
execFileSync('git', ['diff', '--exit-code', '--', 'src', 'dist/types', 'fixtures'], { cwd: root, stdio: 'inherit' })
for (const client of process.argv.slice(2)) {
  const destination = resolve(root, client)
  const manifestPath = join(destination, 'package.json')
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  if (!manifest.dependencies?.['@athenaeum/protocol']) throw new Error(`${client} does not consume @athenaeum/protocol`)
  const vendor = join(destination, 'vendor')
  mkdirSync(vendor, { recursive: true })
  const [packed] = JSON.parse(execFileSync(process.execPath, [npm, 'pack', '--ignore-scripts', '--json', '--pack-destination', vendor], { cwd: root, encoding: 'utf8' }))
  const sha256 = createHash('sha256').update(readFileSync(join(vendor, packed.filename))).digest('hex')
  manifest.dependencies['@athenaeum/protocol'] = `file:vendor/${packed.filename}`
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  writeFileSync(join(vendor, 'protocol-origin.json'), `${JSON.stringify({ repository: 'https://github.com/Hanoryx-North/athenaeum', revision, artifact: packed.filename, sha256, regenerate: 'npm run build && npm run vendor -- <client repository>' }, null, 2)}\n`)
  console.log(`${manifest.name}: ${packed.filename} (${sha256}) from ${revision}`)
}
