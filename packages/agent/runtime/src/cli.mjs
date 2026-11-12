#!/usr/bin/env node
import { createInterface } from 'node:readline'
import { randomUUID } from 'node:crypto'
import { resolve } from 'node:path'
import { stat } from 'node:fs/promises'
import { agentHome, loadConfig, sessionFile } from './config.mjs'
import { inspectSession, openSession, runTurn } from './agent.mjs'
import { learningStatus, LearningWorker } from './learning.mjs'
import { memoryServer } from './memory-service.mjs'

const send = (value) => process.stdout.write(JSON.stringify(value) + '\n')
const argv = process.argv.slice(2)
function help() {
  process.stdout.write(`athenaeum-agent — Athenaeum's coding assistant\n\nUsage: athenaeum-agent [exec|app-server|inspect|memory] [options] [task]\n  -C, --cwd PATH   Selected project directory\n  --session ID     Resume a saved conversation in that directory\n  inspect          Read saved context and memory diagnostics without a model call\n  memory           List background learning job diagnostics without transcripts\n  --json           Emit structured activity in headless mode\n  --yes            Explicitly approve local commands, file edits and MCP calls\n  --ephemeral      Do not persist this headless conversation\n  --help           Show help\n\nConfigure a model and API endpoint in ~/.athenaeum-agent/config.json or Settings.\nCommands run with your privileges. There is no operating-system sandbox.\n`)
}
if (argv.includes('--help') || argv.includes('-h')) { help(); process.exit(0) }
if (argv.includes('--version')) { process.stdout.write('athenaeum-agent 0.1.0\n'); process.exit(0) }

async function server(home, config) {
  let learning = new LearningWorker(home, config), checkingLearning = false, closing = false
  const wakeLearning = async () => {
    if (checkingLearning || closing) return
    checkingLearning = true
    try {
      const latest = await loadConfig(home)
      if (JSON.stringify([latest.backgroundMemory, latest.capture]) !== JSON.stringify([learning.config.backgroundMemory, learning.config.capture])) {
        await learning.close()
        learning = new LearningWorker(home, latest)
      }
      if (!closing) await learning.wake()
    } finally { checkingLearning = false }
  }
  const learningTimer = setInterval(() => { void wakeLearning().catch(() => {}) }, 5000).unref()
  void wakeLearning().catch(() => {})
  const sessions = new Map(), running = new Map(), approvals = new Map()
  const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
  const reply = (id, result) => send({ id, result })
  const emit = (threadId, method, params) => send({ method, params: { threadId, ...params } })
  const stop = () => { for (const run of running.values()) run.controller.abort(); input.close() }
  process.once('SIGTERM', stop); process.once('SIGINT', stop)
  input.on('close', () => { closing = true; clearInterval(learningTimer); void learning.close(); for (const run of running.values()) run.controller.abort() })
  async function handle(request) {
    const { id, method, params = {} } = request
    if (!method) {
      const approval = approvals.get(String(id))
      if (approval) { approvals.delete(String(id)); approval(request.result?.decision === 'accept') }
      return
    }
    try {
      if (method === 'initialized') return
      if (method === 'initialize') { reply(id, { name: 'Athenaeum Agent', version: '0.1.0', agentHome: home }); return }
      if (method === 'memory/read') { reply(id, await learningStatus(home)); return }
      if (method === 'thread/start' || method === 'thread/resume') {
        const session = await openSession(home, params.cwd || process.cwd(), params.sessionId)
        if (sessions.size >= 64 && !sessions.has(session.id)) throw new Error('Session limit reached. Restart the service to load another conversation.')
        sessions.set(session.id, session); reply(id, { thread: { id: session.id } }); return
      }
      if (method === 'thread/read') {
        const session = sessions.get(params.threadId)
        if (!session) throw new Error('Unknown conversation. Resume it before inspection.')
        reply(id, { thread: inspectSession(session) }); return
      }
      if (method === 'turn/interrupt') {
        const run = running.get(params.threadId)
        if (run && run.id === params.turnId) run.controller.abort()
        reply(id, {}); return
      }
      if (method !== 'turn/start') throw new Error(`Unsupported method: ${method}`)
      // Settings may enable or pause learning while chat remains connected.
      config = await loadConfig(home)
      const session = sessions.get(params.threadId)
      if (!session) throw new Error('Unknown conversation.')
      if (running.has(session.id)) throw new Error('A turn is already running in this conversation.')
      if (running.size >= 4) throw new Error('Concurrent turn limit reached.')
      const controller = new AbortController(), turnId = randomUUID()
      running.set(session.id, { id: turnId, controller })
      reply(id, { turn: { id: turnId } })
      const approve = (kind, detail) => new Promise((resolveApproval, reject) => {
        const approvalId = `approval-${randomUUID()}`
        const finish = (accepted) => { clearTimeout(timer); approvals.delete(approvalId); controller.signal.removeEventListener('abort', abort); accepted ? resolveApproval() : reject(new Error('Permission denied.')) }
        const abort = () => finish(false)
        const timer = setTimeout(() => finish(false), 10 * 60 * 1000)
        approvals.set(approvalId, finish)
        if (controller.signal.aborted) { abort(); return }
        controller.signal.addEventListener('abort', abort, { once: true })
        send({ id: approvalId, method: `item/${kind}/requestApproval`, params: { threadId: session.id, turnId, command: kind === 'commandExecution' ? detail : undefined, reason: kind === 'fileChange' ? detail : undefined } })
      })
      try {
        await runTurn({ home, session, config, input: params.input, signal: controller.signal, approve, emit: (method, values) => emit(session.id, method, values) })
        emit(session.id, 'turn/completed', { turn: { id: turnId, status: 'completed' } })
        void wakeLearning().catch(() => {})
      } catch (error) {
        emit(session.id, 'turn/completed', { turn: { id: turnId, status: controller.signal.aborted ? 'interrupted' : 'failed', error: { message: error.message } } })
      } finally { running.delete(session.id) }
    } catch (error) { if (id !== undefined) send({ id, error: { code: -32602, message: error.message } }) }
  }
  input.on('line', (line) => {
    if (Buffer.byteLength(line) > 12 * 1024 * 1024) { send({ error: { message: 'Request exceeds 12 MiB.' } }); return }
    try { const request = JSON.parse(line); if (!request || typeof request !== 'object') throw new Error('Invalid frame'); void handle(request) }
    catch { send({ error: { code: -32700, message: 'Invalid JSON frame.' } }) }
  })
}
async function main() {
  const home = await agentHome(), config = await loadConfig(home)
  if (argv[0] === 'memory-server') { await memoryServer(home); return }
  if (argv[0] === 'app-server') { await server(home, config); return }
  if (argv[0] === 'memory' && argv.length === 1) { send(await learningStatus(home)); return }
  const args = [...argv]
  const inspect = args[0] === 'inspect'
  if (inspect) args.shift()
  if (args[0] === 'exec') args.shift()
  let cwd = process.cwd(), sessionId, yes = false, json = false, ephemeral = false
  const task = []
  for (let index = 0; index < args.length; index++) {
    const arg = args[index]
    if (arg === '--cwd' || arg === '-C' || arg === '--session') {
      const value = args[++index]
      if (!value || value.startsWith('--')) throw new Error(`Missing value for ${arg}.`)
      if (arg === '--session') sessionId = value; else cwd = resolve(value)
    } else if (arg === '--yes') yes = true
    else if (arg === '--json') json = true
    else if (arg === '--ephemeral') ephemeral = true
    else if (arg.startsWith('-')) throw new Error(`Unknown option: ${arg}`)
    else task.push(arg)
  }
  if (inspect) {
    if (!sessionId || task.length || yes || ephemeral) throw new Error('Usage: athenaeum-agent inspect -C PROJECT --session ID [--json]')
    await stat(sessionFile(home, sessionId))
  }
  const session = await openSession(home, cwd, sessionId)
  if (inspect) { send(inspectSession(session)); return }
  const controller = new AbortController()
  process.once('SIGINT', () => controller.abort()); process.once('SIGTERM', () => controller.abort())
  // Ephemeral mode has a private disposable state directory, never writes session history.
  const state = ephemeral ? { ...session, id: `ephemeral-${randomUUID()}` } : session
  const { mkdtemp, rm } = await import('node:fs/promises')
  const { tmpdir } = await import('node:os')
  const transient = ephemeral ? await mkdtemp(resolve(tmpdir(), 'athenaeum-turn-')) : null
  const terminal = task.length ? null : createInterface({ input: process.stdin, output: process.stdout, terminal: process.stdin.isTTY })
  const learning = new LearningWorker(home, config)
  controller.signal.addEventListener('abort', () => { void learning.close() }, { once: true })
  const approve = async (_kind, detail) => {
    if (yes) return
    if (!terminal || !process.stdin.isTTY) throw new Error('Permission denied: use the desktop approval controls, an interactive terminal, or explicitly select --yes.')
    const answer = await new Promise((resolveAnswer) => terminal.question(`\n${detail}\nAllow? [y/N] `, resolveAnswer))
    if (!/^y(es)?$/i.test(answer.trim())) throw new Error('Permission denied.')
  }
  async function turn(prompt) {
    try {
      const result = await runTurn({ home: transient || home, session: state, config, input: [{ type: 'text', text: prompt }], signal: controller.signal, approve,
        emit: (method, params) => { if (json) send({ method, params }); else if (method === 'item/agentMessage/delta') process.stdout.write(params.delta) } })
      if (json) send({ type: 'result', sessionId: state.id, text: result })
      else process.stdout.write('\n')
      // Ephemeral conversations leave no learning jobs or retained transcripts.
      if (!ephemeral) await learning.wake()
    } catch (error) { if (controller.signal.aborted) throw new Error('Turn cancelled.'); throw error }
  }
  try {
    if (task.length) await turn(task.join(' '))
    else {
      process.stdout.write('Athenaeum Agent. Type a task; /quit exits.\n')
      while (!controller.signal.aborted) {
        const prompt = await new Promise((resolvePrompt) => {
          const close = () => resolvePrompt('/quit')
          terminal.once('close', close)
          terminal.question('> ', (value) => { terminal.removeListener('close', close); resolvePrompt(value) })
        })
        if (prompt === '/quit') break
        if (prompt.trim()) { try { await turn(prompt) } catch (error) { process.stderr.write(`${error.message}\n`) } }
      }
    }
  } finally { terminal?.close(); await learning.close(); if (transient) await rm(transient, { recursive: true, force: true }) }
}
main().catch((error) => { process.stderr.write(`Athenaeum Agent: ${error.message}\n`); process.exitCode = 1 })
