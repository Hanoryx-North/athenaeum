import { spawn } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { createHash } from 'node:crypto'
import { createInterface } from 'node:readline'
import { cleanEnvironment } from './tools.mjs'

// Small stdio JSON-RPC client. Knowledge remains behind wiki mcp and its grant checks.
export class McpClient {
  constructor(config, cwd) {
    if (!config.command || typeof config.command !== 'string' || !Array.isArray(config.args || []) || (config.args || []).some((arg) => typeof arg !== 'string')) throw new Error('Invalid MCP command/argv.')
    this.pending = new Map(); this.serial = 0; this.closed = false
    this.child = spawn(config.command, config.args || [], { cwd, env: { ...cleanEnvironment(), ...config.env }, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'] })
    this.child.stderr.resume() // Do not echo potentially secret-bearing server diagnostics.
    this.child.on('error', (error) => this.fail(error))
    this.child.on('exit', () => this.fail(new Error('MCP server stopped.')))
    this.child.stdin.on('error', (error) => this.fail(error))
    let buffered = 0
    this.child.stdout.on('data', (chunk) => {
      buffered += chunk.length
      if (buffered > 4 * 1024 * 1024) this.close()
      if (chunk.includes(10)) buffered = chunk.length - chunk.lastIndexOf(10) - 1
    })
    this.lines = createInterface({ input: this.child.stdout, crlfDelay: Infinity })
    this.lines.on('line', (line) => {
      try {
        const message = JSON.parse(line)
        if (message.method && message.id !== undefined) {
          this.write({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'Interactive server requests are unsupported.' } }); return
        }
        const request = this.pending.get(message.id)
        if (!request) return
        this.pending.delete(message.id)
        if (message.error) request.reject(new Error('MCP request failed. Check the server configuration.'))
        else request.resolve(message.result)
      } catch { this.fail(new Error('Invalid MCP frame.')) }
    })
  }
  write(value) { if (!this.closed) this.child.stdin.write(JSON.stringify(value) + '\n') }
  fail(error) { this.closed = true; for (const request of this.pending.values()) request.reject(error); this.pending.clear() }
  async request(method, params = {}, signal) {
    if (this.closed) throw new Error('MCP server is unavailable.')
    const id = ++this.serial
    return new Promise((resolve, reject) => {
      const abort = () => { this.pending.delete(id); this.write({ jsonrpc: '2.0', method: 'notifications/cancelled', params: { requestId: id, reason: 'Cancelled' } }); finish(reject, signal.reason) }
      const finish = (done, value) => { clearTimeout(timer); signal?.removeEventListener('abort', abort); done(value) }
      const timer = setTimeout(() => { this.pending.delete(id); finish(reject, new Error('MCP request timed out.')) }, 20000)
      this.pending.set(id, { resolve: (value) => finish(resolve, value), reject: (error) => finish(reject, error) })
      if (signal?.aborted) { abort(); return }
      signal?.addEventListener('abort', abort, { once: true })
      this.write({ jsonrpc: '2.0', id, method, params })
    })
  }
  async tools(signal) {
    await this.request('initialize', { protocolVersion: '2024-11-05', capabilities: {}, clientInfo: { name: 'athenaeum-agent', version: '0.1.0' } }, signal)
    this.write({ jsonrpc: '2.0', method: 'notifications/initialized' })
    const tools = []; let cursor
    for (let page = 0; page < 10; page++) {
      const result = await this.request('tools/list', cursor ? { cursor } : {}, signal)
      if (!Array.isArray(result.tools)) throw new Error('MCP tools/list returned an invalid result.')
      tools.push(...result.tools)
      if (tools.length > 128) throw new Error('MCP tool limit exceeded.')
      cursor = result.nextCursor
      if (!cursor) return tools
    }
    throw new Error('MCP tool pagination limit exceeded.')
  }
  close() {
    if (this.closing) return this.closing
    this.fail(new Error('MCP connection closed.')); this.lines.close(); this.child.stdin.end()
    const child = this.child
    this.closing = new Promise((resolve) => {
      if (child.exitCode !== null || child.signalCode !== null || !child.pid) { resolve(); return }
      const timer = setTimeout(() => child.kill(), 500)
      child.once('close', () => { clearTimeout(timer); resolve() })
    })
    return this.closing
  }
}
export async function connectTools(config, cwd, signal) {
  const connections = [], tools = [], handlers = new Map(), diagnostics = []
  const servers = { athenaeum: { command: process.env.ATHENAEUM_BIN || 'wiki', args: ['mcp', '--', '--allow', 'propose', '--caller', 'athenaeum-agent'], required: false }, ...config.mcpServers }
  try {
    for (const [id, server] of Object.entries(servers)) {
      if (!server || server.enabled === false) continue
      const client = new McpClient(server, cwd); connections.push(client)
      const stagedTools = [], stagedHandlers = new Map()
      try {
        for (const tool of await client.tools(signal)) {
          if (!/^[a-zA-Z0-9_-]{1,40}$/.test(id) || !/^[a-zA-Z0-9_-]{1,80}$/.test(tool.name)) throw new Error('Unsupported MCP tool name.')
          const name = id === 'athenaeum' ? tool.name : `${id}__${tool.name}`
          if (handlers.has(name) || stagedHandlers.has(name)) throw new Error('Duplicate MCP tool name.')
          // Never expose wiki review/admin operations, even from a permissive server.
          if (id === 'athenaeum' && !/^wiki_(context|search|show|open|navigate|path|history|status|schema|list|create|link|attach_evidence|repository_info)$/.test(name)) continue
          stagedHandlers.set(name, { client, tool: tool.name, approval: id !== 'athenaeum' })
          stagedTools.push({ type: 'function', name, description: tool.description || name, parameters: tool.inputSchema })
        }
        tools.push(...stagedTools)
        for (const [name, handler] of stagedHandlers) handlers.set(name, handler)
      } catch (error) {
        await client.close()
        if (signal.aborted || server.required !== false) throw error
        diagnostics.push({ server: id, status: 'unavailable' })
      }
    }
    return { tools, handlers, diagnostics, close: () => Promise.all(connections.map((client) => client.close())) }
  } catch (error) { await Promise.all(connections.map((client) => client.close())); throw error }
}

export async function recallContext(connection, task, signal, options = {}) {
  const checkedAt = new Date().toISOString()
  const status = (state) => ({ diagnostic: { status: state, checkedAt }, items: [] })
  signal.throwIfAborted()
  // Only cache absence/errors, never knowledge bodies. Persisting a brief would
  // create another stale knowledge store. Different tasks are always searched.
  const key = createHash('sha256').update(JSON.stringify([options.identity || '', task.trim().slice(0, 4000)])).digest('hex')
  const previous = options.previous
  const ttl = previous?.status === 'empty' ? 60000 : previous?.status === 'unavailable' ? 15000 : 0
  if (previous?.key === key && Date.now() - Date.parse(previous.checkedAt) < ttl) {
    return { diagnostic: { ...previous, cached: true }, items: [] }
  }
  const remember = (state) => ({ diagnostic: { status: state, checkedAt, key }, items: [] })
  const handler = connection.handlers.get('wiki_context')
  if (!handler || handler.approval) return remember('unavailable')
  if (!task.trim()) return status('no_text')
  if (/^(?:ok(?:ay)?|yes|no|thanks|thank you|continue|go ahead|looks good)[.!\s]*$/i.test(task.trim())) return status('no_task')
  const args = { task: task.trim().slice(0, 4000), depth: 1, maxNodes: 5 }
  try {
    const result = await handler.client.request('tools/call', { name: handler.tool, arguments: args }, signal)
    signal.throwIfAborted()
    if (result?.isError) return remember('unavailable')
    const brief = result?.structuredContent?.brief
    if (brief && !brief.nodes?.length && !brief.warnings?.length) return remember('empty')
    const text = result?.content?.filter((part) => part.type === 'text' && typeof part.text === 'string').map((part) => part.text).join('\n')
    if (!text?.trim()) return remember('empty')
    // Preserve the engine's trust wrapper verbatim. Never truncate a wrapper or
    // leak oversized/private server results into the primary conversation.
    if (Buffer.byteLength(text) > 8 * 1024) return status('over_budget')
    const callId = `recall-${randomUUID()}`
    return { diagnostic: { status: 'recalled', checkedAt, ...(Array.isArray(brief?.nodes) ? { nodes: brief.nodes.length } : {}) }, items: [
      { type: 'function_call', call_id: callId, name: 'wiki_context', arguments: JSON.stringify(args) },
      { type: 'function_call_output', call_id: callId, output: text },
    ] }
  } catch (error) {
    if (signal.aborted) throw error
    return remember('unavailable')
  }
}
