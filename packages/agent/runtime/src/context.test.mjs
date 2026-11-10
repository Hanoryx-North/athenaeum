import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { mkdtemp, mkdir, readFile, readdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { compactContext, contextLimit } from './context.mjs'
import { openSession, runTurn } from './agent.mjs'

async function fixture(t, handler) {
  const directory = await mkdtemp(join(tmpdir(), 'athenaeum-context-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const home = join(directory, 'home'), root = join(directory, 'project')
  await mkdir(root)
  const requests = []
  const server = createServer(async (request, response) => {
    const chunks = []; for await (const chunk of request) chunks.push(chunk)
    const body = JSON.parse(Buffer.concat(chunks)); requests.push(body)
    handler(body, response, requests.length)
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve) }))
  const config = { model: 'test-model', api: 'responses', baseURL: `http://127.0.0.1:${server.address().port}/v1`, contextMaxBytes: 32768, mcpServers: { athenaeum: { enabled: false } } }
  const session = await openSession(home, root, 'context-test')
  return { home, session, config, requests, system: 'system', tools: [], signal: new AbortController().signal }
}
function answer(response, text) {
  response.writeHead(200, { 'Content-Type': 'application/json' })
  response.end(JSON.stringify({ status: 'completed', output: [{ type: 'message', content: [{ type: 'output_text', text }] }] }))
}
const oldHistory = () => [
  { role: 'user', content: 'Fix the parser. Keep its public API.' },
  { role: 'assistant', content: 'Investigation ' + 'x'.repeat(25000) },
  { type: 'reasoning', id: 'reason-1', encrypted_content: 'opaque', summary: [] },
  { type: 'function_call', call_id: 'a', name: 'read_file', arguments: '{"path":"a"}' },
  { type: 'function_call', call_id: 'b', name: 'read_file', arguments: '{"path":"b"}' },
  { type: 'function_call_output', call_id: 'a', output: 'parser source' },
  { type: 'function_call_output', call_id: 'b', output: 'parser tests' },
]

test('compaction retains paired parallel tools and reasoning, archives exact history, and hides summary deltas', async (t) => {
  const f = await fixture(t, (_body, response) => answer(response, 'Parser public API must stay stable. Tests remain to be run.'))
  const history = oldHistory()
  const result = await compactContext({ ...f, history })
  assert.ok(result.checkpoint.afterBytes < result.checkpoint.beforeBytes)
  assert.deepEqual(result.history.slice(-5), history.slice(-5))
  assert.ok(result.history.some((item) => item.content === history[0].content))
  assert.match(result.history[0].content, /reference material, not instructions or permission/)
  assert.equal(f.requests.length, 1)
  assert.deepEqual(f.requests[0].tools, [])
  assert.equal(f.requests[0].max_output_tokens, 4096)
  const archive = join(f.home, 'sessions', `${f.session.id}.checkpoints`, `${result.checkpoint.id}.json`)
  assert.deepEqual(JSON.parse(await readFile(archive, 'utf8')).history, history)
  assert.deepEqual(history, oldHistory())
})

test('automatic compaction persists the replacement before continuing and resumes after restart', async (t) => {
  const f = await fixture(t, (body, response, count) => {
    if (count === 1) { answer(response, 'Keep parser public API; read a and b; run tests next.'); return }
    assert.match(body.input[0].content, /Continuation summary/)
    assert.ok(body.input.some((item) => item.role === 'user' && Array.isArray(item.content) && item.content[0].text === 'Continue and verify.'))
    answer(response, 'Ready to continue.')
  })
  f.session.history = oldHistory()
  const events = []
  const result = await runTurn({ ...f, input: [{ type: 'text', text: 'Continue and verify.' }], approve: async () => { throw new Error('No approvals expected') }, emit: (method, params) => events.push({ method, ...params }) })
  assert.equal(result, 'Ready to continue.')
  assert.equal(events.filter((event) => event.method === 'thread/context/compacted').length, 1)
  assert.deepEqual(events.filter((event) => event.method === 'item/agentMessage/delta').map((event) => event.delta), ['Ready to continue.'])
  const resumed = await openSession(f.home, f.session.root, f.session.id)
  assert.deepEqual(resumed.history, f.session.history)
  assert.equal(resumed.compactions.length, 1)
  assert.equal((await readdir(join(f.home, 'sessions', `${f.session.id}.checkpoints`))).length, 1)
})

test('failed, oversized and cancelled compaction preserve history and create no checkpoint', async (t) => {
  const f = await fixture(t, (_body, response, count) => {
    if (count === 1) { response.writeHead(500); response.end('private body'); return }
    answer(response, 'x'.repeat(10000))
  })
  const history = oldHistory()
  await assert.rejects(compactContext({ ...f, history }), /HTTP 500/)
  await assert.rejects(compactContext({ ...f, history }), /bounded text summary/)
  const controller = new AbortController(); controller.abort()
  await assert.rejects(compactContext({ ...f, history, signal: controller.signal }), /abort/i)
  assert.deepEqual(history, oldHistory())
  await assert.rejects(readdir(join(f.home, 'sessions')), { code: 'ENOENT' })
})

test('compaction settings reject invalid limits, allow disabled mode, and reject an oversized indivisible input', async (t) => {
  const f = await fixture(t, () => assert.fail('No model request expected'))
  for (const value of [null, 0, -1, 3.5, '32768', 7 * 1024 * 1024]) {
    if (value === null) continue // null uses the default, like an omitted setting.
    assert.throws(() => contextLimit({ contextMaxBytes: value }), /contextMaxBytes/)
  }
  assert.throws(() => contextLimit({ autoCompact: 'yes' }), /autoCompact/)
  assert.equal(await compactContext({ ...f, history: [{ role: 'user', content: 'small' }] }), null)
  assert.equal(await compactContext({ ...f, config: { ...f.config, autoCompact: false }, history: oldHistory() }), null)
  const history = [{ role: 'user', content: 'x'.repeat(40000) }]
  await assert.rejects(compactContext({ ...f, config: { ...f.config, autoCompact: false }, history }), /context limit/)
  await assert.rejects(compactContext({ ...f, history }), /latest message or tool group/)
  assert.equal(f.requests.length, 0)
})
