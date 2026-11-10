import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, readFile, writeFile, rm, realpath, symlink } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { agentHome, atomicJson, loadConfig, providerConfig, sessionFile } from './config.mjs'
import { command, planPatch, runLocal, workspacePath } from './tools.mjs'
import { openSession } from './agent.mjs'

async function fixture(t) {
  const dir = await mkdtemp(join(tmpdir(), 'athenaeum-runtime-Ω-'))
  t.after(() => rm(dir, { recursive: true, force: true }))
  let root = join(dir, 'project'); await mkdir(root); root = await realpath(root)
  return { dir, root, signal: new AbortController().signal }
}
test('home isolation, canonicalization, invalid config and model validation', async (t) => {
  const { dir } = await fixture(t), home = await agentHome({ ATHENAEUM_AGENT_HOME: join(dir, 'home') })
  assert.equal(home, await agentHome({ ATHENAEUM_AGENT_HOME: home }))
  const path = join(dir, 'file'); await writeFile(path, 'x')
  await assert.rejects(agentHome({ ATHENAEUM_AGENT_HOME: path }))
  await writeFile(join(home, 'config.json'), '{bad')
  await assert.rejects(loadConfig(home, {}), /Cannot read/)
  await writeFile(join(home, 'config.json'), JSON.stringify({ model: 'local-model', api: 'responses' }))
  assert.equal((await loadConfig(home, {})).model, 'local-model')
  assert.equal((await loadConfig(home, { ATHENAEUM_LLM_API_KEY: '', OPENAI_API_KEY: 'must-not-reach-local-provider' })).apiKey, '')
  assert.throws(() => providerConfig({ api: 'responses' }), /Choose a model/)
  assert.throws(() => providerConfig({ model: 'x', api: 'responses', baseURL: 'http://remote.example/v1' }), /HTTPS/)
  assert.throws(() => providerConfig({ model: 'x', api: 'responses', baseURL: 'https://key@example.com' }), /without credentials/)
})
test('file paths reject traversal, private state, accepted wiki writes and symlink escapes', async (t) => {
  const { dir, root } = await fixture(t)
  await symlink(dir, join(root, 'escape'), process.platform === 'win32' ? 'junction' : 'dir')
  await mkdir(join(root, '.athenaeum')); await mkdir(join(root, 'wiki'))
  await symlink(join(root, '.athenaeum'), join(root, 'private-alias'), process.platform === 'win32' ? 'junction' : 'dir')
  await symlink(join(root, 'wiki'), join(root, 'wiki-alias'), process.platform === 'win32' ? 'junction' : 'dir')
  await assert.rejects(workspacePath(root, 'private-alias/knowledge.db'))
  await assert.rejects(workspacePath(root, 'wiki-alias/page.md', true))
  for (const path of ['../secret', 'escape/secret', '.git/config', '.athenaeum/knowledge.db']) await assert.rejects(workspacePath(root, path))
  await assert.rejects(workspacePath(root, 'wiki/accepted.md', true), /wiki proposals/)
  assert.equal(await workspacePath(root, 'src/new.txt', true), join(root, 'src/new.txt'))
})
test('denial prevents command and file side effects', async (t) => {
  const { root, signal } = await fixture(t)
  const context = { root, signal, approve: async () => { throw new Error('Permission denied.') } }
  await assert.rejects(runLocal('write_file', { path: 'denied.txt', content: 'no' }, context), /denied/)
  await assert.rejects(readFile(join(root, 'denied.txt')))
  await assert.rejects(runLocal('exec_command', { cmd: 'echo denied > denied.txt' }, context), /denied/)
  await assert.rejects(readFile(join(root, 'denied.txt')))
})
test('patches validate every hunk and path before writes and preserve concurrent edits', async (t) => {
  const { root, signal } = await fixture(t)
  await writeFile(join(root, 'a.txt'), 'before\n')
  const context = { root, signal, approve: async () => {} }
  const patch = '*** Begin Patch\n*** Update File: a.txt\n@@\n-before\n+after\n*** Add File: b.txt\n+new\n*** End Patch'
  await runLocal('apply_patch', { patch }, context)
  assert.equal(await readFile(join(root, 'a.txt'), 'utf8'), 'after\n')
  assert.equal(await readFile(join(root, 'b.txt'), 'utf8'), 'new\n')
  await assert.rejects(planPatch(root, patch.replace('a.txt', '../a.txt')))
  await assert.rejects(planPatch(root, patch.replace('before', 'missing')))
  await assert.rejects(runLocal('write_file', { path: 'a.txt', content: 'overwrite' }, { ...context, approve: async () => writeFile(join(root, 'a.txt'), 'concurrent') }), /changed while awaiting/)
  assert.equal(await readFile(join(root, 'a.txt'), 'utf8'), 'concurrent')
})
test('read/search/list tools operate on files and report bounded results', async (t) => {
  const { root, signal } = await fixture(t), context = { root, signal }
  await writeFile(join(root, 'unicode.txt'), 'A Ω\nneedle\n')
  assert.equal((await runLocal('read_file', { path: 'unicode.txt' }, context)).text, 'A Ω\nneedle\n')
  assert.deepEqual((await runLocal('list_files', {}, context)).files, ['unicode.txt'])
  assert.equal((await runLocal('search', { query: 'needle' }, context)).matches[0].line, 2)
})
test('command timeout, cancellation, output bounds and credential isolation', async (t) => {
  const { root, signal } = await fixture(t)
  const node = process.platform === 'win32' ? `"${process.execPath}"` : `'${process.execPath.replaceAll("'", "'\\''")}'`
  const timed = await command(`${node} -e "setInterval(()=>{},1000)"`, root, 50, signal)
  assert.equal(timed.timedOut, true)
  const controller = new AbortController()
  const pending = command(`${node} -e "setInterval(()=>{},1000)"`, root, 20000, controller.signal)
  controller.abort(); await pending
  const bounded = await command(`${node} -e "process.stdout.write('x'.repeat(100000))"`, root, 10000, signal)
  assert.equal(bounded.truncated, true); assert.ok(bounded.output.length <= 65536)
  process.env.ATHENAEUM_LLM_API_KEY = 'test-only-secret'
  t.after(() => delete process.env.ATHENAEUM_LLM_API_KEY)
  const clean = await command(`${node} -e "console.log(process.env.ATHENAEUM_LLM_API_KEY || 'absent')"`, root, 10000, signal)
  assert.equal(clean.output.trim(), 'absent')
})
test('sessions persist atomically, reject traversal and bind to one workspace', async (t) => {
  const { dir, root } = await fixture(t), home = join(dir, 'home')
  const session = await openSession(home, root, 'conversation')
  session.history.push({ role: 'user', content: 'hello' })
  await atomicJson(sessionFile(home, session.id), session)
  assert.deepEqual(await openSession(home, root, session.id), session)
  assert.throws(() => sessionFile(home, '../escape'))
  await assert.rejects(openSession(home, dir, session.id), /different workspace/)
})
test('MCP configuration passes argv without a shell, denies review tools and handles cancellation', async (t) => {
  let connection
  t.after(async () => { await connection?.close() })
  const { dir, root } = await fixture(t)
  const { connectTools, McpClient } = await import('./mcp.mjs')
  const fixturePath = join(dir, 'mcp-fixture.cjs')
  await writeFile(fixturePath, `
const input = require('node:readline').createInterface({input:process.stdin});
input.on('line', line => { const m=JSON.parse(line); const reply=result=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\\n');
if(m.method==='initialize') reply({protocolVersion:'2024-11-05',capabilities:{tools:{}}});
if(m.method==='tools/list') reply({tools:[{name:'wiki_context',inputSchema:{type:'object'}},{name:'wiki_accept',inputSchema:{type:'object'}}]});
if(m.method==='tools/call') reply({content:[{type:'text',text:'reference material'}]});
});`)
  const signal = new AbortController().signal
  connection = await connectTools({ mcpServers: { athenaeum: { command: process.execPath, args: [fixturePath] } } }, root, signal)
  assert.deepEqual(connection.tools.map((tool) => tool.name), ['wiki_context'])
  const handler = connection.handlers.get('wiki_context')
  assert.equal((await handler.client.request('tools/call', { name: handler.tool, arguments: {} }, signal)).content[0].text, 'reference material')
  assert.equal(handler.approval, false)
  const controller = new AbortController()
  const pending = handler.client.request('unanswered', {}, controller.signal)
  controller.abort(); await assert.rejects(pending, /abort/i)
  assert.throws(() => new McpClient({ command: '' }, root), /Invalid MCP/)
  await assert.rejects(connectTools({ mcpServers: { athenaeum: { command: join(dir, 'absent') } } }, root, signal))
})
