import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm, realpath } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { command, runLocal, serializeToolOutput, MAX_OUTPUT } from './tools.mjs'

test('file windows preserve line boundaries, Unicode and the continuation offset', async t => {
  const root = await realpath(await mkdtemp(join(tmpdir(), 'athenaeum-ranges-')))
  t.after(() => rm(root, { recursive: true, force: true }))
  const text = Array.from({ length: 450 }, (_, index) => `${index} 日本語\n`).join('')
  await writeFile(join(root, 'source.txt'), text)
  const context = { root, signal: new AbortController().signal }
  let offset = 0, restored = ''
  for (;;) {
    const page = await runLocal('read_file', { path: 'source.txt', offset }, context)
    restored += page.text
    if (!page.truncated) break
    assert.ok(page.nextOffset > offset)
    offset = page.nextOffset
  }
  assert.equal(restored, text)
  await assert.rejects(runLocal('read_file', { path: 'source.txt', limit: 0 }, context), /limit/)
})

test('a failed Unix pipeline remains failed after filtering', { skip: process.platform === 'win32' }, async () => {
  const result = await command('false | cat', tmpdir(), 5000, new AbortController().signal)
  assert.equal(result.exitCode, 1)
  assert.match(result.summary, /code 1/)
})

test('large failing output retains the failure tail, valid JSON, status and a private full log', async t => {
  const root = await mkdtemp(join(tmpdir(), 'athenaeum-tool-output-'))
  t.after(() => rm(root, { recursive: true, force: true }))
  await writeFile(join(root, 'fail.cjs'), `process.stdout.write('START\\n' + '日本語"\\n'.repeat(20000) + '\\nFINAL FAILURE'); process.exitCode = 7`)
  const node = process.platform === 'win32' ? `"${process.execPath}"` : `'${process.execPath.replaceAll("'", "'\\''")}'`
  const result = await command(`${node} fail.cjs`, root, 5000, new AbortController().signal, join(root, 'logs'))
  assert.equal(result.exitCode, 7)
  assert.equal(result.truncated, true)
  assert.match(result.output, /^START/)
  assert.match(result.output, /FINAL FAILURE$/)
  const serialized = serializeToolOutput(result)
  assert.ok(Buffer.byteLength(serialized) <= MAX_OUTPUT)
  const shown = JSON.parse(serialized)
  assert.equal(shown.exitCode, 7)
  assert.match(shown.output, /FINAL FAILURE$/)
  assert.equal(shown.logPath, result.logPath)
  const log = await readFile(shown.logPath, 'utf8')
  assert.ok(Buffer.byteLength(log) > MAX_OUTPUT)
  assert.match(log, /^START/)
  assert.match(log, /FINAL FAILURE$/)
  assert.deepEqual(JSON.parse(serializeToolOutput({ content: [{ type: 'text', text: 'x'.repeat(MAX_OUTPUT) }] })), { error: 'Tool result exceeds the output budget. Request a smaller range or limit.', truncated: true })
})
