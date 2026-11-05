import assert from 'node:assert/strict'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, it } from 'node:test'
import { root } from './fixtures.ts'

const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'))
const targets: { key: string; condition: string; file: string }[] = []
for (const [key, value] of Object.entries<any>(pkg.exports)) {
  if (typeof value === 'string') targets.push({ key, condition: 'default', file: value })
  else for (const [condition, file] of Object.entries<string>(value)) targets.push({ key, condition, file })
}

describe('package.json exports', () => {
  for (const { key, condition, file } of targets) {
    it(`${key} [${condition}] -> ${file} exists`, () => {
      const concrete = file.includes('*') ? file.slice(0, file.indexOf('*')) : file
      assert.ok(existsSync(join(root, concrete)), `${file} is missing`)
    })
  }

  it('every module has a runtime file, a type file and a typed source', () => {
    for (const name of ['index', 'bridge', 'sync', 'library']) {
      for (const file of [`src/${name}.ts`, `src/${name}.js`, `dist/types/${name}.d.ts`]) assert.ok(existsSync(join(root, file)), file)
    }
    const modules = readdirSync(join(root, 'src')).filter((f) => f.endsWith('.ts')).map((f) => f.slice(0, -3)).sort()
    assert.deepEqual(readdirSync(join(root, 'src')).filter((f) => f.endsWith('.js')).map((f) => f.slice(0, -3)).sort(), modules, 'every src/*.ts has exactly one src/*.js')
    assert.deepEqual(readdirSync(join(root, 'dist/types')).filter((f) => f.endsWith('.d.ts')).map((f) => f.slice(0, -5)).sort(), modules, 'every src/*.ts has exactly one dist/types/*.d.ts')
  })

  it('the version constants stay at 1', async () => {
    const protocol: any = await import('@athenaeum/protocol')
    assert.equal(protocol.BRIDGE_PROTOCOL_VERSION, 1)
    assert.equal(protocol.API_VERSION, 1)
    assert.equal(protocol.SYNC_PROTOCOL_VERSION, 1)
    assert.equal(protocol.PORTABLE_FORMAT_VERSION, 1)
  })

  it('serves the same functions through the root and every subpath, with no name defined twice', async () => {
    const root_: any = await import('@athenaeum/protocol')
    const seen = new Map<string, string>()
    for (const sub of ['bridge', 'sync', 'library']) {
      const mod: any = await import(`@athenaeum/protocol/${sub}`)
      for (const [name, value] of Object.entries(mod)) {
        assert.equal(seen.has(name), false, `${name} is exported by both ${seen.get(name)} and ${sub}`)
        seen.set(name, sub)
        assert.strictEqual(root_[name], value, `${name} differs between the root and /${sub}`)
      }
    }
    assert.deepEqual(Object.keys(root_).sort(), [...seen.keys()].sort())
  })

  it('exposes the shared fixtures as a subpath so every consumer reads one set', async () => {
    const paths = await import('@athenaeum/protocol/fixtures/sync/v1/paths.json', { with: { type: 'json' } })
    assert.ok(Array.isArray(paths.default.valid))
  })
})
