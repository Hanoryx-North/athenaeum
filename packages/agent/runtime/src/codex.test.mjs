import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { CodexConnection } from './codex.mjs'

async function fixture(t, mode = 'success') {
  const home = await mkdtemp(join(tmpdir(), 'athenaeum-codex-memory-'))
  const script = join(home, 'server.cjs'), log = join(home, 'requests.jsonl')
  await writeFile(script, `const {appendFileSync}=require('node:fs');require('node:readline').createInterface({input:process.stdin}).on('line',line=>{
    const m=JSON.parse(line);appendFileSync(${JSON.stringify(log)},line+'\\n');const reply=result=>process.stdout.write(JSON.stringify({id:m.id,result})+'\\n');
    if(m.method==='initialize')reply({});
    if(m.method==='account/read')reply({account:${mode === 'signed-out' ? 'null' : '{type:"chatgpt"}'}});
    if(m.method==='config/read')reply({config:{mcp_servers:{privateServer:{}}}});
    if(m.method==='thread/start')reply({thread:{id:'extract'}});
    if(m.method==='turn/interrupt')reply({});
    if(m.method==='turn/start'){reply({turn:{id:'turn'}});${mode === 'exit' ? 'process.exit(1)' : `process.stdout.write(JSON.stringify({method:'item/${mode === 'tool' ? 'started' : 'agentMessage/delta'}',params:{threadId:'extract',${mode === 'tool' ? "item:{type:'commandExecution'}" : "delta:'{\"candidates\":[]}'"}}})+'\\n');process.stdout.write(JSON.stringify({method:'turn/completed',params:{threadId:'extract',turn:{status:'completed'}}})+'\\n')`}}
  });`)
  const connection = new CodexConnection({ command: process.execPath, args: [script] }, home)
  t.after(async () => { await connection.close(); await rm(home, { recursive: true, force: true }) })
  return { connection, home, requests: async () => (await readFile(log, 'utf8')).trim().split('\n').map(JSON.parse) }
}
test('ChatGPT extraction selects Luna explicitly, is ephemeral, and disables configured MCP and shell tools', async (t) => {
  const f = await fixture(t), signal = AbortSignal.timeout(5000)
  await f.connection.initialize(signal)
  const result = await f.connection.extract({ home: f.home, model: 'gpt-6-luna', history: [], system: 'Extract only.', signal })
  assert.equal(result.text, '{"candidates":[]}')
  const requests = await f.requests()
  const start = requests.find((request) => request.method === 'thread/start').params
  assert.equal(start.model, 'gpt-6-luna'); assert.equal(start.ephemeral, true)
  assert.equal(start.sandbox, 'read-only'); assert.equal(start.config['mcp_servers.privateServer.enabled'], false)
  assert.equal(start.config['features.shell_tool'], false)
  assert.equal(requests.find((request) => request.method === 'turn/start').params.model, 'gpt-6-luna')
})
test('signed-out accounts, tool attempts and connection exits fail without model fallback', async (t) => {
  for (const mode of ['signed-out', 'tool', 'exit']) await t.test(mode, async (subtest) => {
    const f = await fixture(subtest, mode), signal = AbortSignal.timeout(5000)
    await assert.rejects(async () => {
      await f.connection.initialize(signal)
      await f.connection.extract({ home: f.home, model: 'gpt-6-luna', history: [], system: 'Extract only.', signal })
    })
    assert.ok((await f.requests()).filter((request) => request.method === 'turn/start').length <= 1)
  })
})
