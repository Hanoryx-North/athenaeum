import { afterEach, expect, test } from 'vitest'
import { cpSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { restoreResourceSymlinks, verifyRelativeSymlinks } from './relay-links.mjs'

const fixtures = []
const fixture = () => {
  const directory = mkdtempSync(join(tmpdir(), 'relay-links-'))
  fixtures.push(directory)
  return directory
}
afterEach(() => { for (const directory of fixtures.splice(0)) rmSync(directory, { recursive: true, force: true }) })

test('restores flattened npm links and keeps them valid after relocation', () => {
  const source = fixture()
  mkdirSync(join(source, 'bin'))
  mkdirSync(join(source, 'lib'))
  writeFileSync(join(source, 'lib', 'npm.js'), 'npm fixture\n')
  symlinkSync('../lib/npm.js', join(source, 'bin', 'npm'))
  const output = fixture()
  cpSync(source, output, { recursive: true, dereference: true })
  restoreResourceSymlinks(source, output)
  const relocated = join(fixture(), 'moved app')
  cpSync(output, relocated, { recursive: true, verbatimSymlinks: true })
  rmSync(source, { recursive: true })
  rmSync(output, { recursive: true })
  verifyRelativeSymlinks(relocated)
  expect(readlinkSync(join(relocated, 'bin', 'npm'))).toBe('../lib/npm.js')
  expect(readFileSync(join(relocated, 'bin', 'npm'), 'utf8')).toBe('npm fixture\n')
})

test.each(['absolute', 'escape', 'dangling'])('rejects %s links', kind => {
  const outer = fixture()
  const output = join(outer, 'output')
  mkdirSync(output)
  writeFileSync(join(outer, 'outside'), 'outside')
  writeFileSync(join(output, 'inside'), 'inside')
  const target = { absolute: join(output, 'inside'), escape: '../outside', dangling: 'missing' }[kind]
  symlinkSync(target, join(output, 'link'))
  expect(() => verifyRelativeSymlinks(output)).toThrow()
})
