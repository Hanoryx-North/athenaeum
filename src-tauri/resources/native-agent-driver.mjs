#!/usr/bin/env node
// Athenaeum desktop protocol adapter. The native runtime owns the agent loop,
// sessions, permissions and coding tools; the desktop owns their presentation.
import { spawn } from 'node:child_process'
import { createInterface } from 'node:readline'

const binary = process.argv[2]
const send = (value) => process.stdout.write(`${JSON.stringify(value)}\n`)
const runtime = spawn(binary, ['app-server'], { windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'], env: { ...process.env, ATHENAEUM_NODE: process.execPath } })
runtime.stderr.on('data', (chunk) => process.stderr.write(chunk))
let serial = 0
let active = null
let initialized = null
let closed = false
const pending = new Map()
const approvals = new Map()
const sessions = new Map()
const write = (value) => runtime.stdin.write(`${JSON.stringify(value)}\n`)
const rpc = (method, params) => new Promise((resolve, reject) => {
  if (closed) { reject(new Error('Athenaeum Agent has stopped. Retry to reconnect.')); return }
  const id = ++serial
  pending.set(id, { resolve, reject })
  write({ id, method, params })
})
function notify(type, data) {
  if (!active) return
  send({ kind: 'notification', requestId: active.requestId, runId: active.runId,
    notification: { method: 'session.event', params: { sessionId: active.sessionId, event: { type, data } } } })
}
function fail(error) {
  closed = true
  for (const request of pending.values()) request.reject(error)
  pending.clear()
  if (active) { active.reject(error); active = null }
}
runtime.on('error', fail)
runtime.on('exit', () => fail(new Error('Athenaeum Agent stopped. Retry to reconnect.')))
runtime.stdin.on('error', fail)
createInterface({ input: runtime.stdout, crlfDelay: Infinity }).on('line', (line) => {
  let message
  try { message = JSON.parse(line) } catch { return }
  if ('id' in message && !message.method) {
    const request = pending.get(message.id)
    if (!request) return
    pending.delete(message.id)
    if (message.error) request.reject(new Error(message.error.message))
    else request.resolve(message.result)
    return
  }
  const params = message.params ?? {}
  if ('id' in message) {
    if (message.method === 'item/commandExecution/requestApproval' || message.method === 'item/fileChange/requestApproval') {
      if (!active) { write({ id: message.id, result: { decision: 'decline' } }); return }
      approvals.set(String(message.id), message.id)
      notify('approval/request', { id: String(message.id),
        title: message.method.includes('commandExecution') ? 'Run this command with your permissions?' : 'Allow these file changes?',
        detail: params.command ? `${params.command}\n\nRuns with your account’s file and network access.` : params.reason || 'The agent needs your approval to continue.' })
    } else {
      // Unsupported interactive requests fail explicitly; they are never approved.
      write({ id: message.id, error: { code: -32601, message: 'This interaction is not supported in the desktop yet. Use the advanced terminal.' } })
    }
    return
  }
  if (!active || (params.threadId && params.threadId !== active.threadId)) return
  switch (message.method) {
    case 'item/agentMessage/delta':
      active.text += params.delta ?? ''
      notify('message/delta', { text: params.delta ?? '' })
      break
    case 'item/started': {
      const item = params.item ?? {}
      if (item.type === 'agentMessage' || item.type === 'reasoning') break
      notify('tool/call', { callId: item.id, name: item.tool || item.type,
        arguments: JSON.stringify({ command: item.command, path: item.changes?.map((c) => c.path).join(', ') }) })
      break
    }
    case 'item/completed': {
      const item = params.item ?? {}
      if (item.type === 'agentMessage') {
        active.messages.set(item.id, item.text ?? '')
      } else if (item.type !== 'reasoning') {
        notify('tool/result', { callId: item.id, message: { toolCallId: item.id,
          isError: item.status === 'failed' || (typeof item.exitCode === 'number' && item.exitCode !== 0),
          content: [{ type: 'text', text: item.aggregatedOutput || item.result?.content?.map((c) => c.text ?? '').join('\n') || item.status || 'Completed' }] } })
      }
      break
    }
    case 'error':
      if (!params.willRetry) active.error = params.error?.message || 'The agent turn failed.'
      break
    case 'turn/completed': {
      const turn = params.turn ?? {}
      if (turn.status === 'failed') active.reject(new Error(turn.error?.message || active.error || 'The agent turn failed.'))
      else active.resolve({ sessionId: active.sessionId,
        finalResponse: [...active.messages.values()].join('\n\n') || active.text,
        cancelled: turn.status === 'interrupted' })
      active = null
      approvals.clear()
      break
    }
  }
})

async function initialize() {
  if (!initialized) initialized = rpc('initialize', {
    clientInfo: { name: 'athenaeum-desktop', title: 'Athenaeum', version: '0.1.0' },
    capabilities: { experimentalApi: true },
  }).then(() => write({ method: 'initialized' }))
  return initialized
}
async function handle(request) {
  try {
    if (request.type === 'shutdown') { runtime.stdin.end(); send({ kind: 'shutdown', requestId: request.requestId }); return }
    if (request.type === 'approval') {
      const id = approvals.get(request.approvalId)
      if (id === undefined) throw new Error('This approval has expired.')
      if (!['accept', 'decline'].includes(request.decision)) throw new Error('Choose Allow or Deny.')
      write({ id, result: { decision: request.decision } })
      approvals.delete(request.approvalId)
      send({ kind: 'approval-accepted', requestId: request.requestId })
      return
    }
    if (request.type === 'cancel') {
      if (active && active.runId === request.runId) active.cancelled = true
      if (active && active.runId === request.runId && active.threadId && active.turnId) {
        await rpc('turn/interrupt', { threadId: active.threadId, turnId: active.turnId })
      }
      send({ kind: 'cancel-accepted', requestId: request.requestId, running: !!active })
      return
    }
    await initialize()
    if (request.type === 'check') {
      send({ kind: 'ready', requestId: request.requestId, workspace: request.projectPath,
        provider: 'Athenaeum Agent', model: process.env.ATHENAEUM_AGENT_MODEL || 'native configuration' })
      return
    }
    if (request.type !== 'run') return
    if (active) throw new Error('An agent turn is already running.')
    if (!request.sessionId || !request.projectPath) throw new Error('Open a project and conversation first.')
    const completion = new Promise((resolve, reject) => {
      active = { ...request, threadId: null, turnId: null, text: '', messages: new Map(), resolve, reject }
    })
    // Attach an immediate rejection handler while thread/start is in flight.
    completion.catch(() => {})
    let threadId = sessions.get(request.sessionId)
    if (!threadId) {
      const result = await rpc('thread/start', { cwd: request.projectPath, sessionId: request.sessionId })
      threadId = result.thread.id
      sessions.set(request.sessionId, threadId)
    }
    active.threadId = threadId
    if (active.cancelled) {
      active.resolve({ sessionId: active.sessionId, finalResponse: '', cancelled: true })
      active = null
      send({ kind: 'cancelled', requestId: request.requestId, runId: request.runId })
      return
    }
    const input = (request.contentBlocks || []).map((block) => block.type === 'image'
      ? { type: 'image', url: `data:${block.mimeType};base64,${block.data}` }
      : { type: 'text', text: block.text || '', text_elements: [] })
    notify('step/start', {})
    const started = await rpc('turn/start', { threadId, input })
    if (active) active.turnId = started.turn.id
    const result = await completion
    send(result.cancelled ? { kind: 'cancelled', requestId: request.requestId, runId: request.runId }
      : { kind: 'result', requestId: request.requestId, runId: request.runId, result })
  } catch (error) {
    if (request.type === 'run') { active = null; approvals.clear() }
    send({ kind: 'error', requestId: request.requestId, message: error.message || String(error) })
  }
}
const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
input.on('line', (line) => {
  try { void handle(JSON.parse(line)) } catch { /* Ignore malformed frames. */ }
})
input.on('close', () => {
  // Let the service close its SQLite handles before the driver exits. Killing
  // the parent immediately can strand native processes and locked state on Windows.
  runtime.stdin.end()
  setTimeout(() => { if (!closed) runtime.kill() }, 5_000).unref()
})
