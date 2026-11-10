import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createServer } from 'node:http'
import { modelTurn } from './provider.mjs'

async function provider(t, handler) {
  const server = createServer(async (request, response) => {
    const chunks = []; for await (const chunk of request) chunks.push(chunk)
    try { handler(JSON.parse(Buffer.concat(chunks)), response, request) }
    catch (error) { response.writeHead(500); response.end(); throw error }
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve) }))
  return { model: 'test-model', baseURL: `http://127.0.0.1:${server.address().port}/v1`, api: 'responses' }
}
const input = [{ role: 'user', content: 'hello' }]
const tools = [{ type: 'function', name: 'read_file', parameters: { type: 'object', properties: { path: { type: 'string' } }, required: ['path'] } }]
function sse(response, values) {
  response.writeHead(200, { 'Content-Type': 'text/event-stream' })
  response.end(values.map((value) => `data: ${JSON.stringify(value)}\n\n`).join(''))
}
test('Responses streams text, retains encrypted reasoning and pairs tool ids', async (t) => {
  const config = await provider(t, (body, response) => {
    assert.equal(body.store, false); assert.deepEqual(body.include, ['reasoning.encrypted_content']); assert.equal(body.tools[0].strict, false)
    sse(response, [
      { type: 'response.output_text.delta', delta: 'Working Ω' },
      { type: 'response.output_item.done', item: { type: 'reasoning', id: 'r1', encrypted_content: 'opaque', summary: [] } },
      { type: 'response.output_item.done', item: { type: 'function_call', name: 'read_file', call_id: 'c1', arguments: '{"path":"a"}' } },
      { type: 'response.completed', response: { output: [{ type: 'function_call', name: 'read_file', call_id: 'c1', arguments: '{"path":"a"}' }] } },
    ])
  })
  const deltas = []
  const result = await modelTurn(config, input, tools, 'system', new AbortController().signal, (value) => deltas.push(value))
  assert.equal(result.text, 'Working Ω'); assert.deepEqual(deltas, ['Working Ω']); assert.equal(result.calls.length, 1)
  assert.equal(result.reasoning[0].encrypted_content, 'opaque'); assert.equal(result.calls[0].call_id, 'c1')
})
test('Chat Completions groups parallel tool calls before their outputs', async (t) => {
  const config = await provider(t, (body, response) => {
    const assistant = body.messages.find((message) => message.tool_calls)
    assert.equal(assistant.tool_calls.length, 2)
    assert.equal(body.messages.filter((message) => message.role === 'tool').length, 2)
    sse(response, [
      { choices: [{ delta: { tool_calls: [{ index: 0, id: 'c3', function: { name: 'read_', arguments: '{"path":' } }] } }] },
      { choices: [{ delta: { tool_calls: [{ index: 0, function: { name: 'file', arguments: '"x"}' } }] }, finish_reason: 'tool_calls' }] },
    ])
  })
  const history = [...input, ...['c1', 'c2'].map((id) => ({ type: 'function_call', call_id: id, name: 'read_file', arguments: '{}' })), ...['c1', 'c2'].map((id) => ({ type: 'function_call_output', call_id: id, output: 'ok' }))]
  const result = await modelTurn({ ...config, api: 'chat-completions' }, history, tools, 'system', new AbortController().signal, () => {})
  assert.deepEqual(result.calls, [{ type: 'function_call', call_id: 'c3', name: 'read_file', arguments: '{"path":"x"}' }])
})
test('Anthropic streams tool input and reports output limits explicitly', async (t) => {
  let calls = 0
  const config = await provider(t, (body, response, request) => {
    assert.equal(request.url, '/v1/messages'); assert.equal(body.tools[0].name, 'read_file')
    if (calls++) { sse(response, [{ type: 'message_delta', delta: { stop_reason: 'max_tokens' } }, { type: 'message_stop' }]); return }
    sse(response, [
      { type: 'content_block_start', index: 0, content_block: { type: 'tool_use', id: 'c1', name: 'read_file' } },
      { type: 'content_block_delta', index: 0, delta: { type: 'input_json_delta', partial_json: '{"path":"a"}' } },
      { type: 'message_stop' },
    ])
  })
  const settings = { ...config, api: 'anthropic' }
  const result = await modelTurn(settings, input, tools, 'system', new AbortController().signal, () => {})
  assert.equal(result.calls[0].arguments, '{"path":"a"}')
  await assert.rejects(modelTurn(settings, input, tools, 'system', new AbortController().signal, () => {}), /output limit/)
})
test('provider errors do not expose body credentials and incomplete streams fail', async (t) => {
  let calls = 0
  const config = await provider(t, (_body, response) => {
    if (calls++ === 0) { response.writeHead(401); response.end('private credential echoed by provider'); return }
    sse(response, [{ type: 'response.output_text.delta', delta: 'partial' }])
  })
  await assert.rejects(modelTurn(config, input, tools, 'system', new AbortController().signal, () => {}), (error) => error.message.includes('401') && !error.message.includes('private'))
  await assert.rejects(modelTurn(config, input, tools, 'system', new AbortController().signal, () => {}), /ended before completion/)
})
test('cancellation aborts the provider request', async (t) => {
  let started
  const ready = new Promise((resolve) => { started = resolve })
  const config = await provider(t, () => started())
  const controller = new AbortController()
  const request = modelTurn(config, input, tools, 'system', controller.signal, () => {})
  await ready; controller.abort()
  await assert.rejects(request, /abort/i)
})

test('output budgets use each provider API field and reject invalid values before network access', async (t) => {
  const fields = ['max_output_tokens', 'max_completion_tokens', 'max_tokens']
  let request = 0
  const config = await provider(t, (body, response) => {
    assert.equal(body[fields[request++]], 2048)
    response.writeHead(200, { 'Content-Type': 'application/json' })
    response.end(JSON.stringify(body.model === 'anthropic' ? { content: [{ type: 'text', text: 'ok' }], stop_reason: 'end_turn' }
      : body.model === 'chat-completions' ? { choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }] }
      : { status: 'completed', output: [{ type: 'message', content: [{ type: 'output_text', text: 'ok' }] }] }))
  })
  for (const api of ['responses', 'chat-completions', 'anthropic']) {
    assert.equal((await modelTurn({ ...config, api, model: api, maxOutputTokens: 2048 }, input, [], 'system', new AbortController().signal, () => {})).text, 'ok')
  }
  for (const maxOutputTokens of [-1, 0, 1.5, '2048', 65537]) await assert.rejects(modelTurn({ ...config, maxOutputTokens }, input, [], 'system', new AbortController().signal, () => {}), /maxOutputTokens/)
  assert.equal(request, 3)
})
test('completed tool work survives a failed model request and resumes with correct error activity', async (t) => {
  const { mkdtemp, mkdir, rm } = await import('node:fs/promises')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { openSession, runTurn } = await import('./agent.mjs')
  const directory = await mkdtemp(join(tmpdir(), 'athenaeum-checkpoint-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const root = join(directory, 'project'), home = join(directory, 'home'); await mkdir(root)
  let requests = 0
  const config = await provider(t, (body, response) => {
    if (requests++ === 0) {
      sse(response, [{ type: 'response.output_item.done', item: { type: 'function_call', call_id: 'c1', name: 'exec_command', arguments: '{"cmd":"exit 7"}' } }, { type: 'response.completed', response: {} }]); return
    }
    if (requests === 2) { response.writeHead(500); response.end(); return }
    assert.ok(body.input.some((item) => item.type === 'function_call_output' && item.output.includes('7')))
    sse(response, [{ type: 'response.output_text.delta', delta: 'Resumed with the failed command result.' }, { type: 'response.completed', response: {} }])
  })
  config.mcpServers = { athenaeum: { enabled: false } }
  const session = await openSession(home, root), emitted = []
  const context = { home, session, config, signal: new AbortController().signal, approve: async () => {}, emit: (method, values) => emitted.push({ method, ...values }) }
  await assert.rejects(runTurn({ ...context, input: [{ type: 'text', text: 'Run a failing command.' }] }), /500/)
  assert.ok(emitted.some((event) => event.item?.id === 'c1' && event.item.status === 'failed'))
  const resumed = await openSession(home, root, session.id)
  assert.equal(resumed.history.at(-1).type, 'function_call_output')
  assert.equal(await runTurn({ ...context, session: resumed, input: [{ type: 'text', text: 'Continue.' }] }), 'Resumed with the failed command result.')
})

test('interruption in a parallel tool group checkpoints completed changes and explicitly closes pending calls', async (t) => {
  const { mkdtemp, mkdir, readFile, rm } = await import('node:fs/promises')
  const { tmpdir } = await import('node:os')
  const { join } = await import('node:path')
  const { openSession, runTurn } = await import('./agent.mjs')
  const directory = await mkdtemp(join(tmpdir(), 'athenaeum-interrupt-'))
  t.after(() => rm(directory, { recursive: true, force: true }))
  const root = join(directory, 'project'), home = join(directory, 'home'); await mkdir(root)
  let requests = 0
  const config = await provider(t, (body, response) => {
    if (requests++ === 0) {
      sse(response, [
        ...[
          { type: 'function_call', call_id: 'edit', name: 'write_file', arguments: '{"path":"done.txt","content":"completed edit"}' },
          { type: 'function_call', call_id: 'command', name: 'exec_command', arguments: '{"cmd":"echo should-not-run"}' },
          { type: 'function_call', call_id: 'later', name: 'write_file', arguments: '{"path":"later.txt","content":"must not exist"}' },
        ].map((item) => ({ type: 'response.output_item.done', item })),
        { type: 'response.completed', response: {} },
      ]); return
    }
    const outputs = body.input.filter((item) => item.type === 'function_call_output')
    assert.equal(outputs.length, 3)
    assert.match(outputs[0].output, /done.txt/)
    assert.match(outputs[1].output, /may have run/)
    assert.match(outputs[2].output, /Not executed/)
    sse(response, [{ type: 'response.output_text.delta', delta: 'Resumed after interruption.' }, { type: 'response.completed', response: {} }])
  })
  config.mcpServers = { athenaeum: { enabled: false } }
  const session = await openSession(home, root), controller = new AbortController()
  const options = { home, session, config, input: [{ type: 'text', text: 'Edit and verify.' }], signal: controller.signal, emit: () => {}, approve: async (kind) => {
    if (kind === 'commandExecution') { controller.abort(); controller.signal.throwIfAborted() }
  } }
  await assert.rejects(runTurn(options), /abort/i)
  assert.equal(await readFile(join(root, 'done.txt'), 'utf8'), 'completed edit')
  await assert.rejects(readFile(join(root, 'later.txt')), { code: 'ENOENT' })
  const resumed = await openSession(home, root, session.id)
  assert.equal(await runTurn({ ...options, session: resumed, signal: new AbortController().signal, input: [{ type: 'text', text: 'Continue.' }] }), 'Resumed after interruption.')
})
