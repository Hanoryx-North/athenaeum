import { providerConfig } from './config.mjs'

const MAX_RESPONSE = 8 * 1024 * 1024
function messages(history) {
  const result = []
  for (const item of history) {
    if (item.type === 'reasoning') continue
    if (item.type === 'function_call') {
      const call = { id: item.call_id, type: 'function', function: { name: item.name, arguments: item.arguments } }
      if (result.at(-1)?.tool_calls) result.at(-1).tool_calls.push(call)
      else result.push({ role: 'assistant', content: null, tool_calls: [call] })
    } else if (item.type === 'function_call_output') result.push({ role: 'tool', tool_call_id: item.call_id, content: item.output })
    else result.push({ role: item.role, content: typeof item.content === 'string' ? item.content : item.content.map((part) => part.type === 'input_image'
      ? { type: 'image_url', image_url: { url: part.image_url } } : { type: 'text', text: part.text || '' }) })
  }
  return result
}
function anthropicMessages(history) {
  const result = []
  for (const item of messages(history)) {
    let role = item.role, content
    if (role === 'tool') { role = 'user'; content = [{ type: 'tool_result', tool_use_id: item.tool_call_id, content: item.content }] }
    else if (item.tool_calls) content = item.tool_calls.map((call) => ({ type: 'tool_use', id: call.id, name: call.function.name, input: JSON.parse(call.function.arguments) }))
    else content = typeof item.content === 'string' ? [{ type: 'text', text: item.content }] : item.content.map((part) => {
      if (part.type === 'text') return part
      const match = /^data:([^;]+);base64,(.+)$/.exec(part.image_url.url)
      if (!match) throw new Error('Only embedded images are supported.')
      return { type: 'image', source: { type: 'base64', media_type: match[1], data: match[2] } }
    })
    const previous = result.at(-1)
    if (previous?.role === role) previous.content.push(...content)
    else result.push({ role, content })
  }
  return result
}
async function* events(response, signal) {
  const reader = response.body.getReader(), decoder = new TextDecoder()
  let buffer = '', bytes = 0
  try {
    while (true) {
      signal.throwIfAborted()
      const { value, done } = await reader.read()
      if (done) break
      bytes += value.byteLength
      if (bytes > MAX_RESPONSE) throw new Error('Model response exceeds 8 MiB.')
      buffer += decoder.decode(value, { stream: true }).replace(/\r/g, '')
      let boundary
      while ((boundary = buffer.indexOf('\n\n')) >= 0) {
        const frame = buffer.slice(0, boundary); buffer = buffer.slice(boundary + 2)
        const data = frame.split('\n').filter((line) => line.startsWith('data:')).map((line) => line.slice(5).trimStart()).join('\n')
        if (!data || data === '[DONE]') continue
        let value
        try { value = JSON.parse(data) } catch { throw new Error('The model sent invalid stream data.') }
        yield value
      }
    }
  } finally { await reader.cancel().catch(() => {}); reader.releaseLock() }
}
export async function modelTurn(settings, history, tools, system, signal, delta) {
  const config = providerConfig(settings), api = config.api
  const path = api === 'responses' ? 'responses' : api === 'anthropic' ? 'messages' : 'chat/completions'
  let body
  if (api === 'responses') body = { model: config.model, instructions: system, input: history, tools: tools.map((tool) => ({ ...tool, strict: false })), include: ['reasoning.encrypted_content'], stream: true, store: false }
  else if (api === 'chat-completions') body = { model: config.model, messages: [{ role: 'system', content: system }, ...messages(history)], tools: tools.map(({ type, ...tool }) => ({ type, function: tool })), stream: true }
  else body = { model: config.model, system, messages: anthropicMessages(history), tools: tools.map((tool) => ({ name: tool.name, description: tool.description, input_schema: tool.parameters })), stream: true, max_tokens: 8192 }
  if (config.maxOutputTokens !== undefined) body[api === 'responses' ? 'max_output_tokens' : api === 'chat-completions' ? 'max_completion_tokens' : 'max_tokens'] = config.maxOutputTokens
  const timeout = AbortSignal.timeout(180000)
  const requestSignal = AbortSignal.any([signal, timeout])
  const headers = { 'Content-Type': 'application/json', Accept: 'text/event-stream' }
  if (config.apiKey) {
    if (api === 'anthropic') headers['x-api-key'] = config.apiKey
    else headers.Authorization = `Bearer ${config.apiKey}`
  }
  if (api === 'anthropic') headers['anthropic-version'] = '2023-06-01'
  const response = await fetch(`${config.baseURL}/${path}`, { method: 'POST', headers, body: JSON.stringify(body), signal: requestSignal, redirect: 'error' })
  // Provider bodies may echo credentials or prompts. Keep errors useful without printing them.
  if (!response.ok) { await response.body?.cancel(); throw new Error(`Model request failed: HTTP ${response.status}. Check the endpoint, model and credentials.`) }
  const calls = new Map(), completed = new Map(), reasoning = new Map()
  let text = '', finished = false
  const append = (value) => { if (value) { text += value; delta(value) } }
  const outputItem = (item) => {
    if (item.type === 'reasoning') reasoning.set(item.id, item)
    else if (item.type === 'function_call') completed.set(item.call_id, { type: 'function_call', call_id: item.call_id, name: item.name, arguments: item.arguments || '{}' })
    else if (item.type === 'message' && !text) for (const part of item.content || []) if (part.text) append(part.text)
  }
  if (response.headers.get('content-type')?.includes('application/json')) {
    const length = Number(response.headers.get('content-length') || 0)
    if (length > MAX_RESPONSE) { await response.body.cancel(); throw new Error('Model response exceeds 8 MiB.') }
    // Some compatible local providers ignore stream=true. Bound their JSON body too.
    const reader = response.body.getReader(); let bytes = 0; const chunks = []
    try { while (true) { const { value, done } = await reader.read(); if (done) break; bytes += value.length; if (bytes > MAX_RESPONSE) throw new Error('Model response exceeds 8 MiB.'); chunks.push(value) } }
    finally { await reader.cancel().catch(() => {}); reader.releaseLock() }
    let value
    try { value = JSON.parse(Buffer.concat(chunks).toString('utf8')) } catch { throw new Error('The model sent invalid JSON.') }
    if (api === 'responses') { for (const item of value.output || []) outputItem(item); finished = value.status !== 'failed' && value.status !== 'incomplete' }
    else if (api === 'anthropic') {
      for (const block of value.content || []) {
        if (block.type === 'text') append(block.text)
        if (block.type === 'tool_use') completed.set(block.id, { type: 'function_call', call_id: block.id, name: block.name, arguments: JSON.stringify(block.input) })
      }
      finished = value.stop_reason !== 'max_tokens'
    } else {
      const choice = value.choices?.[0], message = choice?.message
      append(message?.content)
      for (const call of message?.tool_calls || []) completed.set(call.id, { type: 'function_call', call_id: call.id, name: call.function.name, arguments: call.function.arguments })
      finished = ['stop', 'tool_calls'].includes(choice?.finish_reason)
    }
  } else for await (const event of events(response, requestSignal)) {
    if (event.error || event.type === 'error' || event.type === 'response.failed' || event.type === 'response.incomplete') throw new Error('The model reported a failed or incomplete response.')
    if (api === 'responses') {
      if (event.type === 'response.output_text.delta') append(event.delta)
      if (event.type === 'response.output_item.done') outputItem(event.item)
      if (event.type === 'response.completed') { for (const item of event.response?.output || []) outputItem(item); finished = true }
    } else if (api === 'chat-completions') {
      for (const choice of event.choices || []) {
        append(choice.delta?.content)
        for (const call of choice.delta?.tool_calls || []) {
          const item = calls.get(call.index) || { type: 'function_call', call_id: '', name: '', arguments: '' }
          item.call_id += call.id || ''; item.name += call.function?.name || ''; item.arguments += call.function?.arguments || ''; calls.set(call.index, item)
        }
        if (['stop', 'tool_calls'].includes(choice.finish_reason)) finished = true
      }
    } else {
      if (event.type === 'content_block_start' && event.content_block?.type === 'tool_use') calls.set(event.index, { type: 'function_call', call_id: event.content_block.id, name: event.content_block.name, arguments: '' })
      if (event.type === 'content_block_delta') {
        if (event.delta?.type === 'text_delta') append(event.delta.text)
        if (event.delta?.type === 'input_json_delta') { const call = calls.get(event.index); if (!call) throw new Error('Invalid model tool stream.'); call.arguments += event.delta.partial_json }
      }
      if (event.type === 'message_delta' && event.delta?.stop_reason === 'max_tokens') throw new Error('Model output limit reached. Continue with a smaller task.')
      if (event.type === 'message_stop') finished = true
    }
  }
  if (!finished) throw new Error('The model stream ended before completion. Retry the turn.')
  for (const item of calls.values()) { item.arguments ||= '{}'; completed.set(item.call_id, item) }
  if (completed.size > 32) throw new Error('Model requested too many tools in one step.')
  for (const call of completed.values()) if (!call.call_id || !call.name) throw new Error('Model returned an invalid tool call.')
  return { text, calls: [...completed.values()], reasoning: [...reasoning.values()] }
}
