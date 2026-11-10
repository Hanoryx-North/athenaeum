import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'
import { createInterface } from 'node:readline'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { afterEach, beforeEach, expect, it } from 'vitest'

let root: string
let child: ChildProcessWithoutNullStreams
let serial = 0
let frames: any[]
let waits: Map<string, (frame: any) => void>
function request(type: string, values: Record<string, unknown> = {}): Promise<any> {
  const requestId = String(++serial)
  return new Promise((done, reject) => {
    const timer = setTimeout(() => reject(new Error(`No response to ${type}`)), 5000)
    waits.set(requestId, (frame) => { clearTimeout(timer); done(frame) })
    child.stdin.write(JSON.stringify({ type, requestId, ...values }) + '\n')
  })
}
beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'athenaeum-chatgpt-'))
  const fake = join(root, 'codex.mjs')
  writeFileSync(fake, `import { createInterface } from 'node:readline';
let account = null, thread, turn = 0;
const send = v => process.stdout.write(JSON.stringify(v)+'\\n');
createInterface({input:process.stdin}).on('line', line => {
 const r=JSON.parse(line), p=r.params||{}, reply=v=>send({id:r.id,result:v});
 if(!r.method) { send({method:'item/agentMessage/delta',params:{delta:r.result.decision}}); send({method:'turn/completed',params:{turn:{status:'completed'}}}); return; }
 if(r.method==='initialize') reply({});
 if(r.method==='account/read') reply({account});
 if(r.method==='account/login/start') { reply({loginId:'login-1',authUrl:'https://auth.openai.com/test'}); account={type:'chatgpt',email:'test@example.com',planType:'plus',accessToken:'must-not-leak'}; send({method:'account/login/completed',params:{loginId:'login-1',success:true}}); }
 if(r.method==='account/logout') {account=null;reply({});}
 if(r.method==='account/login/cancel') reply({});
 if(r.method==='thread/start') { thread=p;reply({thread:{id:'thread-1'}}); }
 if(r.method==='turn/start') { reply({turn:{id:'turn-'+(++turn)}}); setTimeout(()=>{ if(p.input[0].text==='hold') return; if(p.input[0].text==='approval') send({id:'approval-1',method:'item/commandExecution/requestApproval',params:{command:'echo test'}}); else {send({method:'item/agentMessage/delta',params:{delta:JSON.stringify(thread)}});send({method:'turn/completed',params:{turn:{status:'completed'}}});} },5); }
 if(r.method==='turn/interrupt') {reply({});send({method:'turn/completed',params:{turn:{status:'interrupted'}}});}
});`)
  frames = []; waits = new Map()
  child = spawn(process.execPath, [resolve('src-tauri/resources/native-agent-driver.mjs'), fake], {
    env: { ...process.env, ATHENAEUM_CHATGPT_MODE: '1', ATHENAEUM_CHATGPT_MCP: JSON.stringify({ command: 'node', args: ['wiki.mjs', 'mcp', '--repo', 'selected-wiki'] }) },
    stdio: 'pipe', windowsHide: true,
  })
  createInterface({ input: child.stdout }).on('line', (line) => {
    const value = JSON.parse(line); frames.push(value)
    if (value.kind !== 'notification') { waits.get(value.requestId)?.(value); waits.delete(value.requestId) }
  })
})
afterEach(async () => {
  child.stdin.end()
  child.kill()
  await new Promise<void>((done) => { if (child.exitCode !== null) done(); else child.once('exit', () => done()) })
  rmSync(root, { recursive: true, force: true })
})

it('requires ChatGPT authentication and returns metadata without tokens', async () => {
  expect((await request('check')).kind).toBe('error')
  expect((await request('run', { sessionId: 'one', projectPath: root })).message).toContain('Sign in with ChatGPT')
  expect((await request('account', { action: 'start' })).result.authUrl).toContain('auth.openai.com')
  const status = (await request('account', { action: 'status' })).result
  expect(status.account).toEqual({ type: 'chatgpt', email: 'test@example.com', planType: 'plus' })
  expect(JSON.stringify(status)).not.toContain('must-not-leak')
  expect((await request('check')).provider).toBe('ChatGPT')
  await request('account', { action: 'logout' })
  expect((await request('account', { action: 'status' })).result.account).toBeNull()
})

it('streams in-app turns with project-scoped wiki tools and explicit approvals', async () => {
  await request('account', { action: 'start' })
  const result = await request('run', { sessionId: 'one', runId: 'ui-one', projectPath: root, contentBlocks: [{ type: 'text', text: 'hello' }] })
  const thread = JSON.parse(result.result.finalResponse)
  expect(thread.cwd).toBe(root)
  expect(thread.approvalPolicy).toBe('on-request')
  expect(thread.sandbox).toBe('workspace-write')
  expect(thread.config['mcp_servers.athenaeum'].args).toContain('selected-wiki')
  const turn = request('run', { sessionId: 'one', runId: 'ui-two', projectPath: root, contentBlocks: [{ type: 'text', text: 'approval' }] })
  await new Promise<void>((done, reject) => {
    const deadline = Date.now() + 3000
    const check = () => frames.some(f => f.notification?.params.event.type === 'approval/request') ? done() : Date.now() > deadline ? reject(new Error('Missing approval')) : setTimeout(check, 10)
    check()
  })
  expect((await request('approval', { approvalId: 'approval-1', decision: 'decline' })).kind).toBe('approval-accepted')
  expect((await turn).result.finalResponse).toBe('decline')
})

it('cancels an in-app task without leaving the driver busy', async () => {
  await request('account', { action: 'start' })
  const turn = request('run', { sessionId: 'one', runId: 'ui-held', projectPath: root, contentBlocks: [{ type: 'text', text: 'hold' }] })
  await new Promise<void>((done) => {
    const check = () => frames.some(f => f.notification?.params.event.type === 'step/start') ? done() : setTimeout(check, 5)
    check()
  })
  expect((await request('cancel', { runId: 'ui-held' })).kind).toBe('cancel-accepted')
  expect((await turn).kind).toBe('cancelled')
  expect((await request('check')).kind).toBe('ready')
})
