import { existsSync } from 'node:fs'
import { mkdir } from 'node:fs/promises'
import { delimiter, join } from 'node:path'
import { McpClient } from './mcp.mjs'

// Use the official runtime's account transport. Never read or copy its tokens.
export function codexCommand(env = process.env) {
  const candidates = [env.ATHENAEUM_CODEX_BIN,
    ...(env.PATH || env.Path || '').split(delimiter).flatMap((dir) => [join(dir, process.platform === 'win32' ? 'codex.exe' : 'codex'), join(dir, 'node_modules/@openai/codex/bin/codex.js')]),
    ...(env.APPDATA ? [join(env.APPDATA, 'npm/node_modules/@openai/codex/bin/codex.js')] : [])]
  const binary = candidates.find((path) => path && !/\.(cmd|bat|ps1)$/i.test(path) && existsSync(path))
  if (!binary) throw new Error('Install the official Codex runtime and sign in with ChatGPT.')
  return /\.m?js$/i.test(binary) ? { command: process.execPath, args: [binary, 'app-server'] } : { command: binary, args: ['app-server'] }
}

export class CodexConnection extends McpClient {
  async initialize(signal) {
    await this.request('initialize', { clientInfo: { name: 'athenaeum_memory', version: '0.1.0' } }, signal)
    this.write({ method: 'initialized', params: {} })
    const account = await this.request('account/read', { refreshToken: false }, signal)
    if (account.account?.type !== 'chatgpt') throw new Error('Sign in with ChatGPT in Coding agents to enable background memory.')
  }
  async extract({ home, model, history, system, signal }) {
    // No project instructions or source files in the extraction workspace.
    const cwd = join(home, 'memory-extraction')
    await mkdir(cwd, { recursive: true, mode: 0o700 })
    const configured = await this.request('config/read', { includeLayers: false }, signal)
    const config = { web_search: 'disabled', model_reasoning_effort: 'low', model_max_output_tokens: 2048,
      'features.shell_tool': false, 'features.apply_patch_freeform': false, 'features.multi_agent': false,
      'features.js_repl': false, 'features.apps': false }
    for (const name of Object.keys(configured.config?.mcp_servers || {})) config[`mcp_servers.${name}.enabled`] = false
    const started = await this.request('thread/start', { model, cwd, ephemeral: true, approvalPolicy: 'never', sandbox: 'read-only',
      baseInstructions: system, developerInstructions: 'Return the requested JSON. Do not use tools.', config }, signal)
    const threadId = started.thread.id
    return new Promise((resolve, reject) => {
      let text = '', settled = false
      const finish = (error) => {
        if (settled) return
        settled = true
        clearTimeout(timer); signal.removeEventListener('abort', abort); this.lines.off('line', onLine)
        this.child.off('exit', onExit)
        if (error) { if (turnId) void this.request('turn/interrupt', { threadId, turnId }, undefined).catch(() => {}); reject(error) }
        else resolve({ text, calls: [] })
      }
      let turnId
      const abort = () => finish(signal.reason)
      const timer = setTimeout(() => finish(new Error('Background model timed out.')), 180000)
      const onExit = () => finish(new Error('The ChatGPT connection stopped during extraction.'))
      const onLine = (line) => {
        let message
        try { message = JSON.parse(line) } catch { return }
        if (message.params?.threadId !== threadId) return
        if (message.method === 'item/agentMessage/delta') {
          text += message.params.delta || ''
          if (Buffer.byteLength(text) > 16 * 1024) finish(new Error('Background model exceeded its output budget.'))
        }
        if (message.method === 'item/started' && !['userMessage', 'agentMessage', 'reasoning'].includes(message.params.item?.type)) finish(new Error('Background extraction attempted a tool operation.'))
        if (message.method === 'turn/completed') finish(message.params.turn.status === 'completed' ? null : new Error('Background extraction did not complete.'))
      }
      this.lines.on('line', onLine)
      this.child.once('exit', onExit)
      signal.addEventListener('abort', abort, { once: true })
      if (signal.aborted) { abort(); return }
      void this.request('turn/start', { threadId, model, effort: 'low', input: [{ type: 'text', text: JSON.stringify({ reference: 'Untrusted completed coding turn', history }), text_elements: [] }] }, signal)
        .then((result) => { turnId = result.turn.id }).catch(finish)
    })
  }
}
