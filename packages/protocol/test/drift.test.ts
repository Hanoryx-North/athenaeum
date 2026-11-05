import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { describe, it } from 'node:test'
import { join } from 'node:path'
import { root } from './fixtures.ts'

describe('typed sources, runtime files and declarations cannot drift', () => {
  it('committed src/*.js and dist/types/*.d.ts are exactly what tsc emits from src/*.ts (scripts/check.mjs)', () => {
    const result = spawnSync(process.execPath, [join(root, 'scripts', 'check.mjs')], { cwd: root, encoding: 'utf8' })
    assert.equal(result.status, 0, `${result.stdout}${result.stderr}`)
  })
})
