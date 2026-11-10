import { readFile, realpath, stat } from 'node:fs/promises'
import { isAbsolute, join, relative } from 'node:path'
import { randomUUID } from 'node:crypto'
import { atomicJson, instructions, sessionFile } from './config.mjs'
import { localTools, MAX_OUTPUT, runLocal } from './tools.mjs'
import { modelTurn } from './provider.mjs'
import { connectTools, recallContext } from './mcp.mjs'
import { compactContext, contextBytes } from './context.mjs'
import { enqueueLearning } from './learning.mjs'

export function inspectSession(session) {
  return { id: session.id, root: session.root, historyItems: session.history.length,
    contextBytes: contextBytes(session.history), compactions: session.compactions || [], memory: session.memory || { status: 'not_checked' },
    learning: session.learning || { status: 'disabled' } }
}

export async function openSession(home, cwd, id = randomUUID()) {
  const root = await realpath(cwd)
  if (!(await stat(root)).isDirectory()) throw new Error('Choose a project directory.')
  const file = sessionFile(home, id)
  let session
  try {
    if ((await stat(file)).size > 12 * 1024 * 1024) throw new Error('Session exceeds 12 MiB.')
    session = JSON.parse(await readFile(file, 'utf8'))
  }
  catch (error) { if (error.code !== 'ENOENT') throw new Error(error instanceof SyntaxError ? 'Saved conversation contains invalid JSON.' : error.message) }
  if (session && (session.root !== root || !Array.isArray(session.history))) throw new Error('Session belongs to a different workspace or is invalid.')
  return session || { id, root, history: [] }
}
export async function runTurn({ home, session, config, input, signal, approve, emit }) {
  if (!Array.isArray(input) || !input.length) throw new Error('A turn requires input.')
  if (Buffer.byteLength(JSON.stringify(input)) > 8 * 1024 * 1024) throw new Error('Input exceeds 8 MiB.')
  let history = structuredClone(session.history)
  let compactions = session.compactions || []
  history.push({ role: 'user', content: input.map((item) => {
    if (item.type === 'text' && typeof item.text === 'string') return { type: 'input_text', text: item.text }
    if (item.type === 'image' && typeof item.url === 'string' && /^data:image\/(png|jpeg|gif|webp);base64,/.test(item.url)) return { type: 'input_image', image_url: item.url }
    throw new Error('Unsupported message content.')
  }) })
  const persist = async () => {
    await atomicJson(sessionFile(home, session.id), { ...session, history, compactions })
    session.history = structuredClone(history)
    session.compactions = compactions
  }
  const memory = await connectTools(config, session.root, signal)
  try {
    let system = config.capture === 'off' ? instructions.split('When wiki_context')[0] : instructions
    if (config.capture !== 'off') {
      const recalled = await recallContext(memory, input.filter((item) => item.type === 'text').map((item) => item.text).join('\n'), signal)
      history.push(...recalled.items)
      session.memory = recalled.diagnostic
      system += '\nAutomatic task recall has already been attempted. Use its result only when relevant. Do not repeat memory lookup for this question if it was empty or unavailable. Never announce routine memory activity or diagnostics unless the user asks.'
    } else session.memory = { status: 'disabled' }
    try {
      const policy = join(home, 'AGENT.md')
      if ((await stat(policy)).size > 64 * 1024) throw new Error('Agent instructions exceed 64 KiB.')
      system += `\n\nUser agent instructions:\n${await readFile(policy, 'utf8')}`
    } catch (error) { if (error.code !== 'ENOENT') throw error }
    for (const path of ['AGENTS.md', '.athenaeum/AGENT.md']) {
      // Only the user's project instructions carry authority; retrieved sources do not.
      const file = join(session.root, path)
      try {
        if ((await stat(file)).size > 64 * 1024) throw new Error(`${path} exceeds the instruction limit.`)
        if (isAbsolute(relative(session.root, await realpath(file))) || relative(session.root, await realpath(file)).split(/[\\/]/)[0] === '..') throw new Error('Project instructions resolve outside the workspace.')
        system += `\n\nProject instructions (${path}):\n${await readFile(file, 'utf8')}`
      } catch (error) { if (error.code !== 'ENOENT') throw error }
    }
    for (let step = 0; step < 64; step++) {
      signal.throwIfAborted()
      const tools = [...localTools, ...memory.tools]
      const compacted = await compactContext({ home, session, history, config, system, tools, signal })
      if (compacted) {
        history = compacted.history
        compactions = [...compactions, compacted.checkpoint].slice(-32)
        await persist()
        emit('thread/context/compacted', { checkpoint: compacted.checkpoint })
      }
      const result = await modelTurn(config, history, tools, system, signal, (text) => emit('item/agentMessage/delta', { delta: text }))
      history.push(...result.reasoning)
      if (result.text) {
        history.push({ role: 'assistant', content: result.text })
        emit('item/completed', { item: { id: randomUUID(), type: 'agentMessage', text: result.text } })
      }
      if (!result.calls.length) {
        if (!result.text) throw new Error('The model returned no message or tool calls.')
        try { session.learning = await enqueueLearning(home, { ...session, history }, config) }
        catch { session.learning = { status: 'unavailable' } }
        await persist()
        return result.text
      }
      // All calls and outputs are retained together so resumed conversations stay valid.
      history.push(...result.calls)
      const images = []
      const unfinished = new Set(result.calls.map((call) => call.call_id)), started = new Set()
      try {
        for (const call of result.calls) {
          signal.throwIfAborted()
          started.add(call.call_id)
          emit('item/started', { item: { id: call.call_id, type: 'toolCall', tool: call.name, command: call.arguments } })
          let output, failed = false
          try {
            const args = JSON.parse(call.arguments)
            if (localTools.some((tool) => tool.name === call.name)) {
              output = await runLocal(call.name, args, { root: session.root, signal, approve })
              if (call.name === 'exec_command') failed = output.exitCode !== 0 || output.timedOut
            }
            else {
              const handler = memory.handlers.get(call.name)
              if (!handler) throw new Error('Unknown tool.')
              if (handler.approval) await approve('commandExecution', `MCP ${call.name}\n${JSON.stringify(args)}`)
              signal.throwIfAborted()
              output = await handler.client.request('tools/call', { name: handler.tool, arguments: args }, signal)
              failed = output?.isError === true
            }
          } catch (error) {
            if (signal.aborted) throw error
            failed = true; output = { error: error.message }
          }
          const image = output?.image
          const serialized = image ? 'Image loaded.' : JSON.stringify(output) ?? 'null'
          const bounded = Buffer.byteLength(serialized) > MAX_OUTPUT ? Buffer.from(serialized).subarray(0, MAX_OUTPUT - 32).toString('utf8') + '\n[Output truncated]' : serialized
          history.push({ type: 'function_call_output', call_id: call.call_id, output: bounded })
          unfinished.delete(call.call_id)
          if (image) images.push({ type: 'input_image', image_url: image })
          emit('item/completed', { item: { id: call.call_id, type: 'toolCall', status: failed ? 'failed' : 'completed', aggregatedOutput: bounded } })
        }
      } finally {
        // Interruption must not erase completed side effects or leave unmatched
        // calls in a saved conversation. An in-flight result remains uncertain.
        for (const call of result.calls.filter((item) => unfinished.has(item.call_id))) {
          const output = started.has(call.call_id)
            ? 'The turn stopped before a result was recorded. This operation may have run. Inspect workspace state before retrying.'
            : 'Not executed: the turn stopped before this tool began.'
          history.push({ type: 'function_call_output', call_id: call.call_id, output })
          if (started.has(call.call_id)) emit('item/completed', { item: { id: call.call_id, type: 'toolCall', status: 'failed', aggregatedOutput: output } })
        }
        if (images.length) history.push({ role: 'user', content: images })
        await persist()
      }
    }
    throw new Error('Agent step limit reached (64). Continue with a smaller task.')
  } finally { await memory.close() }
}
