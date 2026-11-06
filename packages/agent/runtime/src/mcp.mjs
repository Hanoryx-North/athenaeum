import { spawn } from 'node:child_process'
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
    this.fail(new Error('MCP connection closed.')); this.lines.close(); this.child.stdin.end()
    const child = this.child
    setTimeout(() => { if (child.exitCode === null) child.kill() }, 500).unref()
  }
}
export async function connectTools(config, cwd, signal) {
  const connections = [], tools = [], handlers = new Map()
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
          if (id === 'athenaeum' && !/^wiki_(context|search|show|navigate|path|history|status|schema|list|create|link|repository_info)$/.test(name)) continue
          stagedHandlers.set(name, { client, tool: tool.name, approval: id !== 'athenaeum' })
          stagedTools.push({ type: 'function', name, description: tool.description || name, parameters: tool.inputSchema })
        }
        tools.push(...stagedTools)
        for (const [name, handler] of stagedHandlers) handlers.set(name, handler)
      } catch (error) {
        client.close()
        if (signal.aborted || server.required !== false) throw error
        process.stderr.write(`Optional MCP server ${id} unavailable; continuing without it.\n`)
      }
    }
    return { tools, handlers, close: () => connections.forEach((client) => client.close()) }
  } catch (error) { connections.forEach((client) => client.close()); throw error }
}
