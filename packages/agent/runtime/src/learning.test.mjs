import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { mkdtemp, mkdir, readFile, readdir, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { atomicJson, sessionFile } from './config.mjs'
import { enqueueLearning, learningStatus, LearningWorker } from './learning.mjs'

const candidate = () => ({ type: 'gotcha', title: 'Retry budget protects the shared quota', summary: 'Keep retry work bounded to prevent outage amplification.', content: 'The retry budget is three. Broader outage behavior has not been tested.', evidence: [{ path: 'retry.mjs', excerpt: 'export const retryBudget = 3' }] })
async function fixture(t, candidates = [candidate()], { duplicate = false, related = false, tools = true, gate } = {}) {
  const dir = await mkdtemp(join(tmpdir(), 'athenaeum-learning-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  const home = join(dir, 'home'), project = join(dir, 'project'); await mkdir(project)
  const root = await realpath(project)
  await writeFile(join(root, 'retry.mjs'), 'export const retryBudget = 3\n')
  const history = [
    { role: 'user', content: [{ type: 'input_text', text: 'Investigate retry behavior.' }] },
    { type: 'function_call', call_id: 'read', name: 'read_file', arguments: '{"path":"retry.mjs"}' },
    { type: 'function_call_output', call_id: 'read', output: JSON.stringify({ text: 'export const retryBudget = 3\n' }) },
    { role: 'assistant', content: 'Retry work is bounded. Keep the shared quota protected.' },
  ]
  const session = { id: 'learn-test', root, history }
  await atomicJson(sessionFile(home, session.id), session)
  const log = join(dir, 'mcp-log.jsonl'), script = join(dir, 'wiki.cjs')
  await writeFile(script, `
const {appendFileSync}=require('node:fs');require('node:readline').createInterface({input:process.stdin}).on('line',line=>{
const m=JSON.parse(line), reply=result=>process.stdout.write(JSON.stringify({id:m.id,result})+'\\n');
if(m.method==='initialize')reply({});
if(m.method==='tools/list')reply({tools:${JSON.stringify(tools ? ['wiki_search', 'wiki_create', 'wiki_attach_evidence', 'wiki_link', 'wiki_accept'].map((name) => ({ name, inputSchema: { type: 'object' } })) : [])}});
if(m.method==='tools/call'){appendFileSync(${JSON.stringify(log)},JSON.stringify(m.params)+'\\n');
const data=m.params.name==='wiki_search'?{items:${JSON.stringify(duplicate ? [{ node: { id: 'existing', title: candidate().title } }] : related ? [{ node: { id: 'existing', title: 'Retry behavior during maintenance' } }] : [])}}:m.params.name==='wiki_create'?{node:{id:'node_proposed'},pendingReview:true,durable:false}:{evidence:{id:'evidence'}};
reply({structuredContent:data,content:[{type:'text',text:'reference material'}]});}
});`)
  const requests = []
  let requested
  const ready = new Promise((resolve) => { requested = resolve })
  const server = createServer(async (request, response) => {
    const chunks = []; for await (const chunk of request) chunks.push(chunk)
    requests.push(JSON.parse(Buffer.concat(chunks)))
    requested()
    if (gate) await gate
    response.writeHead(200, { 'Content-Type': 'application/json' })
    response.end(JSON.stringify({ status: 'completed', output: [{ type: 'message', content: [{ type: 'output_text', text: JSON.stringify({ candidates }) }] }] }))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve) }))
  const config = { model: 'coding-model', api: 'responses', baseURL: `http://127.0.0.1:${server.address().port}/v1`, backgroundMemory: { enabled: true, model: 'cheap-model' }, mcpServers: { athenaeum: { command: process.execPath, args: [script] } } }
  const calls = async () => { try { return (await readFile(log, 'utf8')).trim().split('\n').map(JSON.parse) } catch (error) { if (error.code === 'ENOENT') return []; throw error } }
  return { home, session, config, requests, calls, ready }
}

test('learning is opt-in, honors capture off, and queues no more than eight bounded jobs', async (t) => {
  const f = await fixture(t)
  assert.equal((await enqueueLearning(f.home, f.session, {})).status, 'disabled')
  assert.equal((await enqueueLearning(f.home, f.session, { ...f.config, capture: 'off' })).status, 'disabled')
  assert.equal((await enqueueLearning(f.home, f.session, { ...f.config, backgroundMemory: { enabled: true } })).status, 'disabled')
  for (let count = 0; count < 8; count++) assert.equal((await enqueueLearning(f.home, f.session, f.config)).status, 'queued')
  assert.equal((await enqueueLearning(f.home, f.session, f.config)).status, 'queue_full')
  assert.equal(f.requests.length, 0)
})

test('separate learning worker verifies sources, searches first, proposes with evidence and retains no completed transcript', async (t) => {
  const f = await fixture(t)
  await enqueueLearning(f.home, f.session, f.config)
  // A new worker reconstructs persisted work, rather than depending on chat state.
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  assert.equal(f.requests.length, 1)
  assert.equal(f.requests[0].model, 'cheap-model')
  assert.equal(f.requests[0].max_output_tokens, 2048)
  assert.deepEqual(f.requests[0].tools, [])
  const calls = await f.calls()
  assert.deepEqual(calls.map((item) => item.name), ['wiki_search', 'wiki_search', 'wiki_create', 'wiki_attach_evidence'])
  assert.match(calls[2].arguments.content, /retry.mjs/)
  assert.equal(calls[3].arguments.node, 'node_proposed')
  assert.equal((await learningStatus(f.home)).jobs[0].status, 'completed')
  assert.ok(!JSON.stringify(await learningStatus(f.home)).includes('Investigate retry'))
  const [name] = await readdir(join(f.home, 'memory-jobs'))
  const saved = JSON.parse(await readFile(join(f.home, 'memory-jobs', name), 'utf8'))
  assert.ok(!saved.history)
  assert.deepEqual(saved.proposed, ['node_proposed'])
  const restarted = new LearningWorker(f.home, f.config)
  await restarted.wake(); await restarted.close()
  assert.equal(f.requests.length, 1)
})

test('learning skips duplicates and never offers review or file execution tools to the model', async (t) => {
  const f = await fixture(t, [candidate()], { duplicate: true })
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  assert.deepEqual((await f.calls()).map((item) => item.name), ['wiki_search', 'wiki_search'])
  assert.deepEqual((await learningStatus(f.home)).jobs[0].proposed, [])
})

test('source references attach exact observed text and related hits do not suppress distinct findings', async (t) => {
  const value = { ...candidate(), evidence: [{ source: 'source_1' }], related: [{ node: 'existing', reason: 'Both constrain retry behavior.' }, { node: 'invented', reason: 'Unsupported.' }] }
  const f = await fixture(t, [value], { related: true })
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  const calls = await f.calls()
  assert.deepEqual(calls.map((item) => item.name), ['wiki_search', 'wiki_search', 'wiki_create', 'wiki_attach_evidence', 'wiki_link'])
  assert.equal(calls[3].arguments.excerpt, 'export const retryBudget = 3')
  assert.equal(calls[4].arguments.to, 'existing')
  const input = JSON.parse(f.requests[0].input[0].content)
  assert.deepEqual(input.sourceObservations, [{ id: 'source_1', path: 'retry.mjs', excerpt: 'export const retryBudget = 3' }])
  assert.ok(!input.history)
  assert.equal((await learningStatus(f.home)).jobs[0].outcome, 'proposed')
})

test('learning refuses stale excerpts, private files, unobserved sources and generated credentials', async (t) => {
  const bad = [
    { ...candidate(), evidence: [{ path: 'retry.mjs', excerpt: 'export const retryBudget = 99' }] },
    { ...candidate(), evidence: [{ path: '.env', excerpt: 'some private source text' }] },
    { ...candidate(), evidence: [{ path: '../outside.mjs', excerpt: 'some outside source text' }] },
    { ...candidate(), content: 'api_key = sk-testcredentialvalue123456789' },
    { ...candidate(), evidence: [{ path: 'unobserved.mjs', excerpt: 'some unseen source text' }] },
  ]
  for (const value of bad) await t.test(value.evidence[0].path + value.content.slice(0, 6), async (subtest) => {
    const f = await fixture(subtest, [value])
    await enqueueLearning(f.home, f.session, f.config)
    const worker = new LearningWorker(f.home, f.config)
    await worker.wake(); await worker.close()
    assert.deepEqual((await f.calls()).map((item) => item.name), ['wiki_search'])
  })
})

test('unavailable proposal capability and turns without source observations spend no model usage', async (t) => {
  for (const mode of ['missing-tools', 'no-source']) await t.test(mode, async (subtest) => {
    const f = await fixture(subtest, [candidate()], { tools: mode !== 'missing-tools' })
    if (mode === 'no-source') f.session.history = f.session.history.filter((item) => item.role)
    await enqueueLearning(f.home, f.session, f.config)
    const worker = new LearningWorker(f.home, f.config)
    await worker.wake(); await worker.close()
    assert.equal(f.requests.length, 0)
    assert.equal((await learningStatus(f.home)).jobs[0].status, mode === 'missing-tools' ? 'failed' : 'completed')
  })
})

test('concurrent workers spend only one extraction request and release ownership after shutdown', async (t) => {
  let release
  const gate = new Promise((resolve) => { release = resolve })
  const f = await fixture(t, [candidate()], { gate })
  await enqueueLearning(f.home, f.session, f.config)
  const first = new LearningWorker(f.home, f.config), second = new LearningWorker(f.home, f.config)
  const processing = first.wake()
  await f.ready
  await second.wake()
  assert.equal(f.requests.length, 1)
  release(); await processing; await first.close(); await second.close()
  assert.equal((await learningStatus(f.home)).jobs[0].status, 'completed')
})

test('shutdown preserves pending learning for a fresh worker and releases the process lock', async (t) => {
  let release
  const gate = new Promise((resolve) => { release = resolve })
  const f = await fixture(t, [candidate()], { gate })
  await enqueueLearning(f.home, f.session, f.config)
  const first = new LearningWorker(f.home, f.config)
  void first.wake(); await f.ready; await first.close()
  assert.equal((await learningStatus(f.home)).jobs[0].status, 'pending')
  release()
  const resumed = new LearningWorker(f.home, f.config)
  await resumed.wake(); await resumed.close()
  assert.equal((await learningStatus(f.home)).jobs[0].status, 'completed')
  assert.equal((await f.calls()).filter((call) => call.name === 'wiki_create').length, 1)
})
