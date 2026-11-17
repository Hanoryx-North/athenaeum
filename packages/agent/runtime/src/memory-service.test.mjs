import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, realpath, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { MemoryService, codexHistory } from './memory-service.mjs'
import { learningStatus } from './learning.mjs'

test('Codex history includes successful file observations and final text, with bounded tool output', () => {
  const history = codexHistory({ items: [
    { type: 'userMessage', content: [{ type: 'text', text: 'Investigate.' }, { type: 'image', url: 'private-image' }] },
    { type: 'agentMessage', phase: 'commentary', text: 'Progress.' },
    { type: 'agentMessage', phase: 'final_answer', text: 'Confirmed result.' },
    { type: 'commandExecution', id: 'failed', status: 'completed', exitCode: 1, command: 'read failed', aggregatedOutput: 'error' },
    { type: 'commandExecution', id: 'read', status: 'completed', exitCode: 0, command: 'Get-Content app.mjs', aggregatedOutput: 'source text\n'.repeat(9000) },
    { type: 'reasoning', text: 'private reasoning' },
  ] })
  const text = JSON.stringify(history)
  assert.match(text, /read failed/); assert.match(text, /Confirmed result/); assert.match(text, /source text/)
  assert.doesNotMatch(text, /private-image|private reasoning|Progress/)
  assert.ok(Buffer.byteLength(text) < 80 * 1024)
})
test('collector captures a fast completed Codex turn once, skips older/interrupted turns, and survives a lost cursor', async (t) => {
  const dir = await mkdtemp(join(tmpdir(), 'athenaeum-collector-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  const home = join(dir, 'home'); await mkdir(home)
  const root = await realpath(dir), now = Date.now()
  const turns = [
    { id: 'new', status: 'completed', completedAt: Math.ceil(now / 1000), items: [{ type: 'userMessage', content: [{ type: 'text', text: 'New task.' }] }] },
    { id: 'old', status: 'completed', completedAt: Math.floor(now / 1000) - 100, items: [] },
    { id: 'interrupted', status: 'interrupted', completedAt: Math.ceil(now / 1000), items: [] },
  ]
  const service = new MemoryService(home)
  service.config = { backgroundMemory: { enabled: true, model: 'gpt-6-luna', wikiRoot: root, enabledAt: new Date(now - 1000).toISOString() } }
  service.connection = async () => ({ request: async (method) => method === 'thread/list'
    ? { data: [{ id: 'thread', cwd: root, updatedAt: Math.ceil(now / 1000) }] } : { data: turns } })
  await service.collect(); await service.collect()
  assert.equal((await learningStatus(home)).jobs.length, 1)
  await rm(join(home, 'memory-codex-cursor.json'))
  await service.collect()
  const jobs = (await learningStatus(home)).jobs
  assert.equal(jobs.length, 1); assert.equal(jobs[0].source, 'codex'); assert.equal(jobs[0].model, 'gpt-6-luna')
})

test('session collection failure does not block queued intake and remains visible in diagnostics', async () => {
  const service = new MemoryService('/unused')
  service.config = { backgroundMemory: { enabled: true, includeCodex: true, model: 'memory-model' } }
  service.collect = async () => { throw new Error('Private transport detail') }
  let processed = 0
  service.worker = { wake: async () => { processed++ } }
  await service.run()
  assert.equal(processed, 1)
  assert.match(service.error, /could not read Codex sessions/)
  assert.doesNotMatch(service.error, /Private transport detail/)
})
