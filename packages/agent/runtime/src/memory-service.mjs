import { createHash } from 'node:crypto'
import { readFile, realpath } from 'node:fs/promises'
import { isAbsolute, join, relative } from 'node:path'
import { createInterface } from 'node:readline'
import { atomicJson, loadConfig, sessionFile } from './config.mjs'
import { CodexConnection, codexCommand } from './codex.mjs'
import { enqueueLearning, learningEnabled, learningStatus, LearningWorker } from './learning.mjs'

export function codexHistory(turn) {
  const history = []
  let budget = 80 * 1024
  const append = (value) => {
    const bytes = Buffer.byteLength(JSON.stringify(value))
    if (bytes > budget) return
    budget -= bytes; history.push(value)
  }
  for (const item of turn.items || []) {
    if (item.type === 'userMessage') append({ role: 'user', content: [{ type: 'input_text', text: (item.content || []).filter((part) => part.type === 'text').map((part) => part.text).join('\n').slice(0, 12000) }] })
    if (item.type === 'agentMessage' && item.phase !== 'commentary') append({ role: 'assistant', content: item.text.slice(0, 12000) })
    if (item.type === 'commandExecution' && item.status === 'completed' && item.exitCode === 0 && typeof item.aggregatedOutput === 'string') {
      const output = item.aggregatedOutput.slice(0, 48000)
      const pair = [
        { type: 'function_call', call_id: item.id, name: 'exec_command', arguments: JSON.stringify({ cmd: item.command }) },
        { type: 'function_call_output', call_id: item.id, output: JSON.stringify({ exitCode: 0, output }) },
      ]
      if (Buffer.byteLength(JSON.stringify(pair)) <= budget) pair.forEach(append)
    }
    if (item.type === 'fileChange' && item.status === 'completed') for (const [index, change] of (item.changes || []).entries()) {
      if (!change.path || typeof change.diff !== 'string') continue
      append({ type: 'function_call', call_id: `${item.id}-${index}`, name: 'apply_patch', arguments: '{}' })
      append({ type: 'function_call_output', call_id: `${item.id}-${index}`, output: JSON.stringify({ changed: [change.path], diff: change.diff.slice(0, 16000) }) })
    }
  }
  return history
}

export class MemoryService {
  constructor(home) { this.home = home; this.controller = new AbortController(); this.state = 'idle'; this.error = null }
  async start() {
    this.config = await loadConfig(this.home)
    this.worker = new LearningWorker(this.home, this.config)
    this.timer = setInterval(() => { void this.run() }, 15000).unref()
    void this.run()
  }
  async configure(settings) {
    if (!settings || typeof settings.enabled !== 'boolean' || typeof settings.includeCodex !== 'boolean' || typeof settings.model !== 'string' || !settings.model.trim() || settings.model.length > 100 || typeof settings.wikiRoot !== 'string') throw new Error('Choose a wiki and a background model.')
    const wikiRoot = await realpath(settings.wikiRoot)
    await readFile(join(wikiRoot, '.athenaeum/config.json'), 'utf8').catch(async (error) => {
      // Older vaults have repository metadata rather than config.json.
      if (error.code !== 'ENOENT') throw error
      const { stat } = await import('node:fs/promises')
      if (!(await stat(join(wikiRoot, '.athenaeum'))).isDirectory()) throw error
    })
    const saved = JSON.parse(await readFile(join(this.home, 'config.json'), 'utf8').catch((error) => { if (error.code === 'ENOENT') return '{}'; throw error }))
    const previous = saved.backgroundMemory
    const resumed = settings.enabled && (!previous?.enabled || previous.wikiRoot !== wikiRoot || settings.includeCodex && !previous.includeCodex)
    const backgroundMemory = { enabled: settings.enabled, includeCodex: settings.includeCodex, model: settings.model.trim(), wikiRoot, backend: 'chatgpt',
      enabledAt: resumed ? new Date().toISOString() : previous?.enabledAt || new Date().toISOString() }
    // Validate account/model before enabling paid background work. Never substitute a model.
    if (backgroundMemory.enabled) {
      const connection = await this.connection()
      const models = await connection.request('model/list', { limit: 100 }, this.controller.signal)
      if (!models.data?.some((model) => model.model === backgroundMemory.model || model.id === backgroundMemory.model)) throw new Error('The selected background model is unavailable on your ChatGPT connection.')
    }
    await this.worker.close()
    await this.running
    await atomicJson(join(this.home, 'config.json'), { ...saved, backgroundMemory })
    this.config = await loadConfig(this.home)
    this.worker = new LearningWorker(this.home, this.config)
    this.error = null
    void this.run()
    return this.status()
  }
  async connection() {
    if (!this.codex || this.codex.closed) {
      this.codex = new CodexConnection(codexCommand(), this.home)
      try { await this.codex.initialize(this.controller.signal) }
      catch (error) { await this.codex.close(); throw error }
    }
    return this.codex
  }
  async collect(threadId) {
    const settings = this.config.backgroundMemory
    const connection = await this.connection()
    const ledgerPath = join(this.home, 'memory-codex-cursor.json')
    let ledger
    try { ledger = JSON.parse(await readFile(ledgerPath, 'utf8')) }
    catch (error) { if (error.code !== 'ENOENT') throw error; ledger = { seen: {} } }
    const threads = threadId ? [(await connection.request('thread/read', { threadId }, this.controller.signal)).thread]
      : (await connection.request('thread/list', { limit: 30, sortKey: 'updated_at' }, this.controller.signal)).data
    for (const thread of threads || []) {
      if (!thread.cwd || thread.ephemeral || !threadId && thread.updatedAt * 1000 < Date.parse(settings.enabledAt)) continue
      const turns = await connection.request('thread/turns/list', { threadId: thread.id, limit: 8, itemsView: 'full' }, this.controller.signal)
      const completed = (turns.data || []).filter((turn) => turn.status === 'completed').reverse()
      const seen = new Set(ledger.seen[thread.id] || [])
      for (const turn of completed) {
        if (seen.has(turn.id)) continue
        // Completion timestamps cover fast new threads and turns that were
        // already running when learning was enabled. UUIDv7 is an older-runtime fallback.
        const completedAt = turn.completedAt ? turn.completedAt * 1000 : /^[a-f0-9]{8}-[a-f0-9]{4}-7/i.test(turn.id) ? parseInt(turn.id.replaceAll('-', '').slice(0, 12), 16) : 0
        if (!threadId && completedAt < Date.parse(settings.enabledAt)) { seen.add(turn.id); continue }
        const root = await realpath(thread.cwd)
        const id = `codex-${createHash('sha256').update(`${thread.id}:${turn.id}`).digest('hex').slice(0, 40)}`
        const history = codexHistory(turn)
        // Normalize absolute file-change paths against the source workspace.
        for (const item of history) if (item.type === 'function_call_output') {
          const value = JSON.parse(item.output)
          if (value.changed) { value.changed = value.changed.map((path) => (isAbsolute(path) ? relative(root, path) : path).replaceAll('\\', '/')); item.output = JSON.stringify(value) }
        }
        await atomicJson(sessionFile(this.home, id), { id, root, history: [] })
        const queued = await enqueueLearning(this.home, { id, root, history, source: 'codex' }, this.config)
        if (queued.status === 'queue_full') break
        seen.add(turn.id)
      }
      ledger.seen[thread.id] = [...seen].slice(-32)
      await atomicJson(ledgerPath, ledger)
    }
  }
  run(threadId) {
    if (this.running) return threadId ? this.running.then(() => this.run(threadId)) : this.running
    if (!learningEnabled(this.config)) return Promise.resolve()
    this.running = (async () => {
      this.state = 'running'; this.error = null
      try {
        if (this.config.backgroundMemory.includeCodex || threadId) await this.collect(threadId)
        await this.worker.wake()
        this.lastRunAt = new Date().toISOString()
      } catch { this.error = 'Background memory could not connect or read sessions. Check your ChatGPT connection and retry.' }
      finally { this.state = 'idle'; this.running = null }
    })()
    return this.running
  }
  async status() {
    const settings = this.config.backgroundMemory || { enabled: false, includeCodex: false, model: 'gpt-6-luna' }
    return { settings, active: learningEnabled(this.config), state: this.state, error: this.error,
      knowledgeCli: process.env.ATHENAEUM_BIN || 'wiki', lastRunAt: this.lastRunAt || null, ...await learningStatus(this.home) }
  }
  async close() { clearInterval(this.timer); this.controller.abort(); await this.worker.close(); await this.codex?.close(); await this.running }
}

export async function memoryServer(home) {
  const service = new MemoryService(home)
  await service.start()
  const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
  let serial = Promise.resolve()
  const send = (value) => process.stdout.write(JSON.stringify(value) + '\n')
  input.on('line', (line) => {
    if (Buffer.byteLength(line) > 16384) return
    let request
    try { request = JSON.parse(line) } catch { return }
    serial = serial.then(async () => {
      try {
        if (request.type === 'shutdown') { input.close(); return }
        if (request.type !== 'memory') throw new Error('Unsupported memory operation.')
        if (request.action === 'configure') await service.configure(request.settings)
        else if (request.action === 'run') { await service.run(request.threadId) }
        else if (request.action !== 'status') throw new Error('Choose configure, status or run.')
        send({ kind: 'result', requestId: request.requestId, result: await service.status() })
      } catch (error) { send({ kind: 'error', requestId: request.requestId, message: error.message }) }
    })
  })
  input.on('close', () => { void service.close() })
}
