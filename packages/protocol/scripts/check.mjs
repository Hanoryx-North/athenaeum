// Fails when the committed runtime files or type declarations are not exactly what `tsc` emits from
// src/*.ts, when the tests no longer typecheck against the sources, or when `exports` points at a
// file that does not exist. The fix for the first is always `npm run build`.
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const tsc = fileURLToPath(import.meta.resolve('typescript/bin/tsc'))
const cache = join(root, 'node_modules', '.cache', 'protocol-check')
const problems = []

if (!existsSync(tsc)) {
  console.error('typescript is not installed; run `npm install` first')
  process.exit(2)
}

function run(label, args) {
  const result = spawnSync(process.execPath, [tsc, ...args], { cwd: root, encoding: 'utf8' })
  if (result.status !== 0) problems.push(`${label} failed:\n${(result.stdout + result.stderr).trim()}`)
  return result.status === 0
}

rmSync(cache, { recursive: true, force: true })
mkdirSync(cache, { recursive: true })
const emitted = run('tsc emit', ['-p', 'tsconfig.json', '--outDir', join(cache, 'js'), '--declarationDir', join(cache, 'types')])

const list = (dir, suffix) => (existsSync(dir) ? readdirSync(dir).filter((name) => name.endsWith(suffix)).sort() : [])
const sources = list(join(root, 'src'), '.ts').map((name) => name.slice(0, -3))

if (emitted) {
  for (const name of sources) {
    for (const [committed, fresh] of [
      [join(root, 'src', `${name}.js`), join(cache, 'js', `${name}.js`)],
      [join(root, 'dist', 'types', `${name}.d.ts`), join(cache, 'types', `${name}.d.ts`)],
    ]) {
      const label = committed.slice(root.length + 1)
      if (!existsSync(committed)) problems.push(`${label} is missing`)
      else if (readFileSync(committed, 'utf8') !== readFileSync(fresh, 'utf8')) problems.push(`${label} differs from what tsc emits from src/${name}.ts`)
    }
  }
  for (const name of list(join(root, 'src'), '.js').map((n) => n.slice(0, -3))) {
    if (!sources.includes(name)) problems.push(`src/${name}.js has no src/${name}.ts`)
  }
  for (const name of list(join(root, 'dist', 'types'), '.d.ts').map((n) => n.slice(0, -5))) {
    if (!sources.includes(name)) problems.push(`dist/types/${name}.d.ts has no src/${name}.ts`)
  }
}

run('typecheck of tests', ['-p', 'tsconfig.test.json'])

const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
const targets = (value) => (typeof value === 'string' ? [value] : Object.values(value).flatMap(targets))
for (const target of targets(pkg.exports)) {
  const concrete = target.includes('*') ? target.slice(0, target.indexOf('*')) : target
  if (!existsSync(join(root, concrete))) problems.push(`package.json exports points at ${target}, which does not exist`)
}

rmSync(cache, { recursive: true, force: true })
if (problems.length) {
  console.error(`protocol check failed:\n- ${problems.join('\n- ')}\n\nRun \`npm run build\` to regenerate src/*.js and dist/types/*.d.ts.`)
  process.exit(1)
}
console.log(`protocol check ok: ${sources.length} modules, runtime files and type declarations match tsc output`)
