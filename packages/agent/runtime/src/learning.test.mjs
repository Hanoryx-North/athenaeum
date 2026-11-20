import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { mkdtemp, mkdir, readFile, readdir, realpath, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { atomicJson, sessionFile } from './config.mjs'
import { enqueueLearning, enqueueText, captureText, learningStatus, LearningWorker } from './learning.mjs'

const candidate = () => ({ type: 'gotcha', title: 'Retry budget protects the shared quota', summary: 'Keep retry work bounded to prevent outage amplification.',
  lesson: { recommendation: 'Keep the shared retry budget at three attempts.', rationale: 'Unbounded retries amplify outages and consume the shared quota.',
    whenToUse: 'When changing retry behavior in this project.', validation: 'The configured budget was inspected; outage behavior has not been tested.', limits: 'Do not infer behavior under concurrent failures from this constant alone.' },
  evidence: [{ path: 'retry.mjs', excerpt: 'export const retryBudget = 3' }] })
const textCandidate = (overrides = {}) => ({
  type: 'procedure', title: candidate().title, summary: candidate().summary,
  content: 'Keep the retry budget bounded so an outage cannot exhaust the shared quota.',
  evidence: [{ source: 'text_1_1' }], ...overrides,
})

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
    const selected = typeof candidates === 'function' ? candidates(requests.at(-1)) : candidates
    response.end(JSON.stringify({ status: 'completed', output: [{ type: 'message', content: [{ type: 'output_text', text: JSON.stringify({ candidates: selected }) }] }] }))
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
  assert.equal(f.requests[0].max_output_tokens, 4096)
  assert.deepEqual(f.requests[0].tools, [])
  const calls = await f.calls()
  assert.deepEqual(calls.map((item) => item.name), ['wiki_search', 'wiki_search', 'wiki_create', 'wiki_attach_evidence'])
  assert.match(calls[2].arguments.content, /## Why it matters/); assert.doesNotMatch(calls[2].arguments.content, /Workspace:|Sources:/)
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
    { ...candidate(), lesson: { ...candidate().lesson, rationale: 'api_key = sk-testcredentialvalue123456789' } },
    { ...candidate(), evidence: [{ path: 'unobserved.mjs', excerpt: 'some unseen source text' }] },
    { ...candidate(), evidence: { source: 'source_1' } },
  ]
  for (const value of bad) await t.test((value.evidence[0]?.path || 'invalid-shape') + value.title.slice(0, 6), async (subtest) => {
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


test('failed commands support debugging lessons without pretending to verify source files', async (t) => {
  const value = { ...candidate(), evidence: [{ source: 'command_1' }] }
  const f = await fixture(t, [value])
  f.session.history = [f.session.history[0],
    { type: 'function_call', call_id: 'test', name: 'exec_command', arguments: JSON.stringify({ cmd: 'npm test -- retry' }) },
    { type: 'function_call_output', call_id: 'test', output: JSON.stringify({ exitCode: 1, output: 'Assertion failed: retries exceeded budget.' }) },
    { role: 'assistant', content: 'The retry test exposed a missing retry bound.' }]
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  const calls = await f.calls()
  assert.equal(calls.find((call) => call.name === 'wiki_attach_evidence').arguments.kind, 'external_source')
  assert.match(calls.at(-1).arguments.note, /Exit: 1/)
  const input = JSON.parse(f.requests[0].input[0].content)
  assert.equal(input.sourceObservations[0].exitCode, 1)
})

test('a substantial coding turn retains late-file architectural reasoning, failures and genuine tool evidence', async (t) => {
  const f = await fixture(t, (request) => {
    const input = JSON.parse(request.input[0].content)
    const source = input.sourceObservations.find((item) => item.path === 'boundary-11.rs')
    assert.match(source.excerpt, /Only reviewed rules may enter the compiler/)
    assert.match(source.excerpt, /struct ReviewedRule/)
    assert.equal(input.coverage.suppliedBlocks, 12)
    assert.ok(input.sourceObservations.some((item) => item.exitCode === 1))
    assert.equal(input.frictionSignals.length, 1)
    assert.doesNotMatch(JSON.stringify(input), /private credential value/)
    return [{ ...candidate(), title: 'Reviewed rule types protect compiler boundaries',
      lesson: { recommendation: 'Require ReviewedRule at the compiler API boundary.',
        rationale: 'Separating generated proposals from reviewed rules prevents accidental enforcement.',
        whenToUse: 'When extending this project’s rule compiler or memory proposal integration.',
        validation: 'The API was inspected; the failing compile test exposed raw proposal acceptance.',
        limits: 'The type boundary does not prove the quality of the reviewed rule.' }, evidence: [{ source: source.id }] }]
  })
  const history = [f.session.history[0]]
  for (let index = 0; index < 12; index++) {
    const path = `boundary-${index}.rs`
    const text = index === 11
      ? '// Only reviewed rules may enter the compiler; generated proposals remain reference data.\npub struct ReviewedRule(String);\npub fn compile(rule: ReviewedRule) {}\n'
      : `// Separate the parsing and validation stage for component ${index}.\npub fn validate_${index}() -> bool { true }\n`
    await writeFile(join(f.session.root, path), text)
    history.push({ type: 'function_call', call_id: `read-${index}`, name: 'read_file', arguments: JSON.stringify({ path }) },
      { type: 'function_call_output', call_id: `read-${index}`, output: JSON.stringify({ text }) })
  }
  for (const [index, command] of ['cargo test compiler', 'cargo test compiler -- --nocapture', 'cargo test compiler --lib'].entries()) {
    history.push({ type: 'function_call', call_id: `cmd-${index}`, name: 'exec_command', arguments: JSON.stringify({ cmd: command }) },
      { type: 'function_call_output', call_id: `cmd-${index}`, output: JSON.stringify({ exitCode: index === 2 ? 0 : 1, output: index === 2 ? 'Compiler boundary test passed.' : 'Compile test failed: raw proposals accepted by compiler.' }) })
  }
  history.push({ type: 'function_call', call_id: 'private', name: 'exec_command', arguments: JSON.stringify({ cmd: 'cat .env' }) },
    { type: 'function_call_output', call_id: 'private', output: JSON.stringify({ exitCode: 0, output: 'private credential value' }) },
    { role: 'assistant', content: 'Introduced a reviewed rule type at the compiler boundary after reproducing unsafe raw proposal acceptance.' })
  f.session.history = history
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  assert.equal((await learningStatus(f.home)).jobs[0].proposed.length, 1)
  const calls = await f.calls()
  assert.equal(calls.find((call) => call.name === 'wiki_attach_evidence').arguments.file, 'boundary-11.rs')
})

test('text intake is opt-in, bounded, deduplicated and tied to the selected wiki', async (t) => {
  const f = await fixture(t)
  const config = { ...f.config, capture: 'off', backgroundMemory: { ...f.config.backgroundMemory, enabled: false, wikiRoot: f.session.root } }
  const document = { title: 'High trust engineering', scope: 'universal', text: 'Use type boundaries and regression tests for recurring mistakes.', wikiRoot: f.session.root }
  await assert.rejects(enqueueText(f.home, document, {}), /Configure a separate/)
  await assert.rejects(enqueueText(f.home, { ...document, text: 'x'.repeat(65537) }, config), /64 KiB/)
  await assert.rejects(enqueueText(f.home, { ...document, text: 'password = abcdefghijklmnop' }, config), /credentials/)
  await assert.rejects(enqueueText(f.home, { ...document, wikiRoot: f.home }, config), /another wiki/)
  assert.equal((await enqueueText(f.home, { text: document.text, wikiRoot: f.session.root }, config)).status, 'queued')
  assert.equal((await enqueueText(f.home, document, config)).status, 'queued')
  assert.equal((await enqueueText(f.home, document, config)).status, 'already_queued')
  assert.equal(f.requests.length, 0)
})

test('an explicitly submitted document runs while automatic session learning stays off', async (t) => {
  const f = await fixture(t, [textCandidate()])
  await enqueueLearning(f.home, f.session, f.config)
  const [name] = await readdir(join(f.home, 'memory-jobs'))
  const file = join(f.home, 'memory-jobs', name)
  const job = JSON.parse(await readFile(file, 'utf8'))
  job.document = { title: 'Reviewed engineering reference', text: 'Bound retries to avoid amplifying an outage and exhausting a shared quota.', scope: 'project' }
  delete job.history
  await atomicJson(file, job)
  // Leave a second ordinary coding job pending: pausing capture must protect it.
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, { ...f.config, capture: 'off', backgroundMemory: { ...f.config.backgroundMemory, enabled: false } })
  await worker.wake(); await worker.close()
  const jobs = (await learningStatus(f.home)).jobs
  assert.equal(f.requests.length, 1)
  assert.equal(jobs.filter((item) => item.status === 'pending').length, 1)
  assert.equal(jobs.filter((item) => item.status === 'completed' && item.proposed.length === 1).length, 1)
})

test('long text is analyzed completely, resumes at its checkpoint and proposes reviewable lessons with separate evidence', async (t) => {
  const f = await fixture(t, [textCandidate()])
  const calls = []
  const client = { request: async (_method, params) => {
    calls.push(params)
    return { structuredContent: params.name === 'wiki_search' ? { items: [] } : params.name === 'wiki_create' ? { node: { id: 'lesson' }, pendingReview: true, durable: false } : { evidence: {} } }
  } }
  const connection = { handlers: new Map(['wiki_search', 'wiki_create', 'wiki_attach_evidence', 'wiki_link'].map((tool) => [tool, { tool, client }])) }
  const text = ('Enforce dependency boundaries with types and tests. Preserve CLI errors and measure unnecessary retries.\n\n').repeat(280) + 'Final unique lesson: verify the application at runtime.'
  const job = { root: f.session.root, model: 'cheap-model', inputHash: 'source-hash', document: { title: 'Two engineering talks', text, scope: 'universal' } }
  let checkpoints = 0
  const result = await captureText(job, f.config, connection, new AbortController().signal, async () => { checkpoints++ })
  const inputs = f.requests.map((request) => JSON.parse(request.input[0].content))
  assert.equal(inputs.flatMap((input) => input.sourceObservations.map((source) => source.excerpt)).join(''), text)
  assert.ok(checkpoints >= 3)
  assert.equal(result.extraction.completedChunks, result.extraction.totalChunks)
  // The fixed fixture refers to chunk one's evidence. Subsequent chunks must
  // reject that reference rather than attaching the wrong source.
  assert.equal(result.proposed.length, 1)
  assert.ok(result.extraction.unsupported >= 2)
  assert.equal(calls.find((call) => call.name === 'wiki_create').arguments.scope, 'universal')
  assert.doesNotMatch(calls.find((call) => call.name === 'wiki_create').arguments.content, /source-hash|Sources:/)
  assert.match(calls.find((call) => call.name === 'wiki_attach_evidence').arguments.note, /not verified project behavior/)
  const before = f.requests.length
  await captureText(job, f.config, connection, new AbortController().signal)
  assert.equal(f.requests.length, before)
})

test('pasted knowledge applies selectors per lesson and links only known, supported targets', async (t) => {
  const reason = 'The retry limit protects the quota described by the validation procedure.'
  const f = await fixture(t, [
    textCandidate({ type: 'rule', scope: 'project', technologies: ['node'], stages: ['coding'], related: [
      { candidate: 2, reason }, { node: 'invented', reason },
    ] }),
    textCandidate({ title: 'Check the quota during outage tests', scope: 'universal', stages: ['verification'], related: [
      { candidate: 1, reason }, { node: 'existing', reason: 'The established outage checklist covers this test.' },
    ] }),
    textCandidate({ title: 'Keep the design background separate', related: [{ candidate: 3, reason }] }),
  ])
  const text = 'In this Node project, bound retries. In every project, check the quota during outage tests. Keep the design background separate.'
  const job = { root: f.session.root, inputHash: 'source-hash', document: { title: 'Pasted text', text } }
  const calls = []
  let created = 0
  const client = { request: async (_method, params) => {
    calls.push(params)
    if (params.name === 'wiki_search') return { structuredContent: { items: params.arguments.query === text ? [{ node: { id: 'existing', title: 'Outage checklist' } }] : [] } }
    if (params.name === 'wiki_create') return { structuredContent: { node: { id: `lesson-${++created}` }, pendingReview: true, durable: false } }
    return { structuredContent: {} }
  } }
  const connection = { handlers: new Map(['wiki_search', 'wiki_create', 'wiki_attach_evidence', 'wiki_link'].map(tool => [tool, { tool, client }])) }

  const result = await captureText(job, f.config, connection, new AbortController().signal)

  assert.deepEqual(result.proposed, ['lesson-1', 'lesson-2', 'lesson-3'])
  const proposals = calls.filter(call => call.name === 'wiki_create').map(call => call.arguments)
  const links = calls.filter(call => call.name === 'wiki_link').map(call => call.arguments)
  assert.deepEqual(proposals.map(({ type, scope, applicability }) => [type, scope, applicability]), [
    ['rule', 'project', { core: false, projects: [job.root], technologies: ['node'], stages: ['coding'] }],
    ['procedure', 'universal', { core: false, projects: [], technologies: [], stages: ['verification'] }],
    ['procedure', 'project', { core: false, projects: [job.root], technologies: [], stages: [] }],
  ])
  assert.ok(calls.findLastIndex(call => call.name === 'wiki_attach_evidence') < calls.findIndex(call => call.name === 'wiki_link'))
  assert.deepEqual(links.map(({ from, to }) => [from, to]), [['lesson-1', 'lesson-2'], ['lesson-2', 'existing']])
})

test('memory verifies observed source beyond the default file-read window and preserves project selectors', async t => {
  const value = { ...candidate(), stages: ['debugging'], technologies: ['node'], evidence: [{ source: 'source_1' }] }
  const f = await fixture(t, [value])
  await writeFile(join(f.session.root, 'retry.mjs'), '\n'.repeat(300) + 'export const retryBudget = 3\n')
  f.config.memoryContext = { project: 'stable-project' }
  await enqueueLearning(f.home, f.session, f.config)
  const worker = new LearningWorker(f.home, f.config)
  await worker.wake(); await worker.close()
  const proposal = (await f.calls()).find(call => call.name === 'wiki_create')
  assert.deepEqual(proposal.arguments.applicability, { core: false, projects: ['stable-project'], technologies: ['node'], stages: ['debugging'] })
})
