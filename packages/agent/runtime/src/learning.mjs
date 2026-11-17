import { createHash, randomUUID } from 'node:crypto'
import { createServer } from 'node:net'
import { readFile, readdir, realpath, stat } from 'node:fs/promises'
import { isAbsolute, join, relative } from 'node:path'
import { atomicJson, sessionFile } from './config.mjs'
import { stages } from './project-context.mjs'
import { ENGINEERING_EXTRACT, MAX_CANDIDATES, sensitive, privatePath, lessonContent, textChunks, commandObservations, frictionSignals } from './engineering-memory.mjs'
import { modelTurn } from './provider.mjs'
import { connectTools } from './mcp.mjs'
import { readWorkspaceText } from './tools.mjs'
import { CodexConnection, codexCommand } from './codex.mjs'

const MAX_JOB = 128 * 1024
export const EXTRACT = ENGINEERING_EXTRACT

export function learningEnabled(config) {
  return config.capture !== 'off' && config.backgroundMemory?.enabled === true &&
    memoryModelConfigured(config)
}
export function memoryModelConfigured(config) {
  return typeof config.backgroundMemory?.model === 'string' && !!config.backgroundMemory.model.trim()
}
const directory = (home) => join(home, 'memory-jobs')
async function files(home) {
  try { return (await readdir(directory(home))).filter((name) => /^[a-f0-9-]{36}\.json$/.test(name)).sort() }
  catch (error) { if (error.code === 'ENOENT') return []; throw error }
}
async function readJob(home, name) {
  const path = join(directory(home), name)
  if ((await stat(path)).size > MAX_JOB) throw new Error('Memory job exceeds its input budget.')
  return JSON.parse(await readFile(path, 'utf8'))
}
function metadata(job) {
  return { id: job.id, sessionId: job.sessionId, createdAt: job.createdAt, status: job.status,
    ...(job.model ? { model: job.model } : {}), ...(job.source ? { source: job.source } : {}),
    ...(job.outcome ? { outcome: job.outcome } : {}), ...(job.extraction ? { extraction: job.extraction } : {}),
    ...(job.finishedAt ? { finishedAt: job.finishedAt } : {}), ...(job.proposed ? { proposed: job.proposed } : {}),
    ...(job.failureStage ? { failureStage: job.failureStage } : {}),
    ...(job.failureOperation ? { failureOperation: job.failureOperation } : {}), ...(job.failureCode ? { failureCode: job.failureCode } : {}) }
}
export async function learningStatus(home) {
  const jobs = []
  for (const name of await files(home)) {
    try { jobs.push(metadata(await readJob(home, name))) }
    catch { jobs.push({ id: name.slice(0, -5), status: 'invalid' }) }
  }
  return { jobs: jobs.sort((a, b) => (b.createdAt || '').localeCompare(a.createdAt || '')) }
}
async function workerLock(home) {
  // An OS-owned listener prevents concurrent workers from billing the same job.
  // Windows pipes and Linux abstract sockets disappear on process exit. Other
  // systems use a loopback port; a collision defers work without losing the job.
  const hash = createHash('sha256').update(home).digest('hex')
  const address = process.platform === 'win32' ? { path: `\\\\.\\pipe\\athenaeum-learning-${hash}` }
    : process.platform === 'linux' ? { path: `\0athenaeum-learning-${hash}` }
    : { host: '127.0.0.1', port: 20000 + parseInt(hash.slice(0, 8), 16) % 40000, exclusive: true }
  const server = createServer((socket) => socket.destroy()).unref()
  const acquired = await new Promise((resolve, reject) => {
    server.once('error', (error) => { if (['EADDRINUSE', 'EACCES'].includes(error.code)) resolve(false); else reject(error) })
    server.listen(address, () => resolve(true))
  })
  return acquired ? () => new Promise((resolve) => server.close(resolve)) : null
}
export async function enqueueLearning(home, session, config) {
  if (!learningEnabled(config)) return { status: 'disabled' }
  const jobs = (await learningStatus(home)).jobs
  if (session.source === 'codex' && jobs.some((job) => job.sessionId === session.id)) return { status: 'already_queued' }
  const pending = jobs.filter((job) => job.status === 'pending')
  if (pending.length >= 8) return { status: 'queue_full' }
  const start = session.history.findLastIndex((item) => item.role === 'user' && Array.isArray(item.content) && item.content.some((part) => part.type === 'input_text'))
  const history = session.history.slice(Math.max(0, start)).filter((item) => item.type !== 'reasoning').map((item) => {
    if (!Array.isArray(item.content)) return item
    return { ...item, content: item.content.filter((part) => part.type === 'input_text') }
  })
  const id = randomUUID(), job = { id, sessionId: session.id, root: session.root, projectId: config.memoryContext?.project, history,
    wikiRoot: config.backgroundMemory.wikiRoot, source: session.source || 'athenaeum',
    model: config.backgroundMemory.model, createdAt: new Date().toISOString(), status: 'pending' }
  if (Buffer.byteLength(JSON.stringify(job)) > MAX_JOB - 4096) return { status: 'over_budget' }
  await atomicJson(join(directory(home), `${id}.json`), job)
  return { status: 'queued', jobId: id }
}

function candidatesFrom(result) {
  if (result.calls.length || Buffer.byteLength(result.text) > 16 * 1024) throw new Error('Invalid knowledge extraction.')
  const value = JSON.parse(result.text)
  if (!value || !Array.isArray(value.candidates) || value.candidates.length > MAX_CANDIDATES) throw new Error('Invalid knowledge extraction.')
  return value.candidates
}
async function verifiedCandidate(candidate, root, observed, commands, signal) {
  if (!candidate || !['decision', 'gotcha', 'procedure', 'fact'].includes(candidate.type) ||
      typeof candidate.title !== 'string' || !candidate.title.trim() || candidate.title.length > 200 ||
      typeof candidate.summary !== 'string' || !candidate.summary.trim() || candidate.summary.length > 1000 ||
      typeof candidate.content !== 'string' || candidate.content.length > 4000 ||
      !Array.isArray(candidate.evidence) || !candidate.evidence.length || candidate.evidence.length > 3 ||
      sensitive(JSON.stringify(candidate))) return null
  const evidence = []
  for (const item of candidate.evidence) {
    if (!item || typeof item.path !== 'string' || typeof item.excerpt !== 'string' ||
        item.excerpt.trim().length < 12 || item.excerpt.length > 1200 || item.path.includes('\\') ||
        isAbsolute(item.path) || item.path.split('/').includes('..') || privatePath(item.path)) return null
    if (!observed.has(item.path) && !commands.some(({ command, output }) => command.includes(item.path) && output.includes(item.excerpt))) return null
    try {
      const text = await readWorkspaceText(root, item.path)
      if (!text.includes(item.excerpt)) return null
      evidence.push({ path: relative(root, join(root, item.path)).replaceAll('\\', '/'), excerpt: item.excerpt })
    } catch (error) { if (signal.aborted) throw error; return null }
  }
  return { ...candidate, title: candidate.title.trim(), evidence }
}
async function call(connection, name, args, signal) {
  const handler = connection.handlers.get(name)
  if (!handler || handler.approval) throw new Error('Proposal-only memory tools are unavailable.')
  const result = await handler.client.request('tools/call', { name: handler.tool, arguments: args }, signal)
  if (result?.isError || !result?.structuredContent) {
    const code = result?.structuredContent?.error?.code
    const error = new Error(`Knowledge operation ${name} failed.`)
    error.memoryOperation = name
    error.memoryCode = typeof code === 'string' && /^[a-z_]{1,60}$/.test(code) ? code : 'invalid_response'
    throw error
  }
  return result.structuredContent
}

function validCandidate(candidate) {
  return candidate && ['decision', 'gotcha', 'procedure', 'fact'].includes(candidate.type) &&
    typeof candidate.title === 'string' && candidate.title.trim().length >= 12 && candidate.title.length <= 200 &&
    typeof candidate.summary === 'string' && candidate.summary.trim().length >= 24 && candidate.summary.length <= 600 &&
    typeof candidate.content === 'string' && !!candidate.content && candidate.content.length <= 4000 &&
    Array.isArray(candidate.evidence) && candidate.evidence.length >= 1 && candidate.evidence.length <= 3 &&
    (candidate.stages === undefined || (Array.isArray(candidate.stages) && candidate.stages.length <= 6 && candidate.stages.every(stage => stages.includes(stage)))) &&
    (candidate.technologies === undefined || (Array.isArray(candidate.technologies) && candidate.technologies.length <= 32 && candidate.technologies.every(value => typeof value === 'string' && value.trim() && Buffer.byteLength(value) <= 300 && !value.includes('\0')))) &&
    !sensitive(JSON.stringify(candidate))
}

async function extractKnowledge(job, config, input, signal) {
  // A saved job never silently changes to the coding model or to a new wiki.
  const model = job.model || config.backgroundMemory.model
  const budget = AbortSignal.any([signal, AbortSignal.timeout(180000)])
  if (config.backgroundMemory.backend === 'chatgpt') {
    const codex = new CodexConnection(codexCommand(), job.root)
    try {
      await codex.initialize(budget)
      return await codex.extract({ home: config.learningHome, model, history: input, system: EXTRACT, signal: budget })
    } finally { await codex.close() }
  }
  return modelTurn({ ...config, model, maxOutputTokens: 4096 },
    [{ role: 'user', content: JSON.stringify(input) }], [], EXTRACT, budget, () => {})
}

async function proposeCandidate(candidate, job, connection, signal) {
  const created = await call(connection, 'wiki_create', { type: candidate.type, title: candidate.title.trim(),
    summary: candidate.summary.trim(), content: candidate.content, scope: job.document?.scope || 'project',
    applicability: { core: job.document?.core === true, projects: job.document?.scope === 'universal' ? [] : [(job.projectId || job.root).replaceAll('\\', '/')],
      technologies: candidate.technologies || [], stages: candidate.stages || [] } }, signal)
  if (!created.pendingReview || created.durable !== false || typeof created.node?.id !== 'string') throw new Error('Knowledge server did not confirm a reviewable proposal.')
  return created
}

export async function enqueueText(home, document, config) {
  // Explicit intake is authorization for this document only. It does not require
  // enabling collection of coding sessions or changing the capture setting.
  if (!memoryModelConfigured(config)) throw new Error('Configure a separate memory model in Coding agents first.')
  if (!document || typeof document.title !== 'string' || !document.title.trim() || document.title.length > 200 ||
      !['project', 'universal'].includes(document.scope) || (document.core !== undefined && typeof document.core !== 'boolean')) throw new Error('Provide a title and choose project or universal applicability.')
  textChunks(document.text) // Validate the entire input before queueing or model usage.
  if (sensitive(document.text) || sensitive(document.title)) throw new Error('The text may contain credentials. Remove them before submitting.')
  const root = config.backgroundMemory.wikiRoot
  if (!root) throw new Error('Select a memory wiki before submitting text.')
  if (typeof document.wikiRoot !== 'string' || await realpath(document.wikiRoot) !== await realpath(root)) throw new Error('Background memory is connected to another wiki. Select this project in Coding agents before submitting.')
  const inputHash = createHash('sha256').update(JSON.stringify([root, document.scope, document.core === true, document.title.trim(), document.text])).digest('hex')
  const jobs = (await learningStatus(home)).jobs
  for (const name of await files(home)) {
    const job = await readJob(home, name)
    if (job.inputHash === inputHash && ['pending', 'completed'].includes(job.status)) return { status: 'already_queued', jobId: job.id }
  }
  if (jobs.filter((job) => job.status === 'pending').length >= 8) throw new Error('The memory queue is full. Process pending work first.')
  const id = randomUUID(), sessionId = `intake-${id}`
  const job = { id, sessionId, root, wikiRoot: root, document: { title: document.title.trim(), text: document.text, scope: document.scope, core: document.core === true }, inputHash,
    source: 'text', model: config.backgroundMemory.model, createdAt: new Date().toISOString(), status: 'pending' }
  // Leave room for checkpoint titles/proposal ids even when JSON escaping
  // expands otherwise small source text.
  if (Buffer.byteLength(JSON.stringify(job)) > MAX_JOB - 16 * 1024) throw new Error('The encoded input exceeds the job budget. Split it explicitly.')
  await atomicJson(sessionFile(home, sessionId), { id: sessionId, root, history: [] })
  await atomicJson(join(directory(home), `${id}.json`), job)
  return { status: 'queued', jobId: id }
}

export async function captureText(job, config, connection, signal, checkpoint = async () => {}) {
  for (const name of ['wiki_search', 'wiki_create', 'wiki_attach_evidence']) {
    if (!connection.handlers.has(name) || connection.handlers.get(name).approval) throw new Error('Proposal-only memory tools are unavailable.')
  }
  const chunks = textChunks(job.document.text)
  const proposed = job.proposed ||= []
  const extraction = job.extraction ||= { candidates: 0, unsupported: 0, duplicates: 0, completedChunks: 0, totalChunks: chunks.length }
  const titles = new Set(job.titles || [])
  for (let index = extraction.completedChunks; index < chunks.length; index++) {
    signal.throwIfAborted()
    const chunk = chunks[index]
    const related = await call(connection, 'wiki_search', { query: job.document.title, limit: 5 }, signal)
    if (!Array.isArray(related.items)) throw new Error('Invalid knowledge search response.')
    const relatedKnowledge = related.items.map((hit) => ({ id: hit.node?.id, title: hit.node?.title, summary: hit.node?.summary })).filter((node) => node.id && node.title && !sensitive(JSON.stringify(node)))
    // Every character is analyzed. The model chooses evidence by stable block
    // id, so it cannot invent a quotation or lose a tail through summarization.
    const sourceObservations = textChunks(chunk.text, 1200).map((part, n) => ({ id: `text_${index + 1}_${n + 1}`, excerpt: part.text }))
    const result = await extractKnowledge(job, config, { reference: 'User-submitted document; advice is not project-verified',
      title: job.document.title, scope: job.document.scope, chunk: index + 1, totalChunks: chunks.length,
      previousContext: job.document.text.slice(Math.max(0, chunk.start - 1000), chunk.start),
      contextPolicy: 'Previous context explains split concepts. Extract only lessons supported by sourceObservations in the current chunk.',
      sourceObservations, relatedKnowledge }, signal)
    for (const value of candidatesFrom(result)) {
      extraction.candidates++
      const evidence = Array.isArray(value?.evidence) ? value.evidence.map((item) => sourceObservations.find((source) => source.id === item?.source)) : []
      const candidate = { ...value, content: lessonContent(value), evidence }
      if (!validCandidate(candidate) || evidence.some((item) => !item)) { extraction.unsupported++; continue }
      const title = candidate.title.trim().toLowerCase()
      const existing = await call(connection, 'wiki_search', { query: candidate.title, limit: 5 }, signal)
      if (!Array.isArray(existing.items)) throw new Error('Invalid knowledge search response.')
      if (titles.has(title) || existing.items.some((hit) => hit.node?.title?.trim().toLowerCase() === title)) { extraction.duplicates++; continue }
      const created = await proposeCandidate(candidate, job, connection, signal)
      proposed.push(created.node.id); titles.add(title)
      for (const item of evidence) await call(connection, 'wiki_attach_evidence', { node: created.node.id,
        excerpt: item.excerpt, kind: 'external_source', note: `Submitted source: ${job.document.title}. SHA-256: ${job.inputHash}. Character range: ${chunk.start}-${chunk.end}. Reference advice, not verified project behavior.` }, signal)
      if (connection.handlers.has('wiki_link')) for (const link of (Array.isArray(value.related) ? value.related.slice(0, 2) : [])) {
        if (!relatedKnowledge.some((node) => node.id === link?.node) || typeof link.reason !== 'string' || !link.reason.trim() || link.reason.length > 1000) continue
        await call(connection, 'wiki_link', { from: created.node.id, type: 'related_to', to: link.node, note: link.reason }, signal)
      }
    }
    extraction.completedChunks = index + 1
    job.titles = [...titles]
    await checkpoint()
  }
  return { proposed, extraction, outcome: proposed.length ? 'proposed' : extraction.candidates ? 'discarded' : 'no_candidates' }
}
export async function capture(job, config, connection, signal) {
  // Check every required capability before spending any model usage.
  for (const name of ['wiki_search', 'wiki_create', 'wiki_attach_evidence']) {
    if (!connection.handlers.has(name) || connection.handlers.get(name).approval) throw new Error('Proposal-only memory tools are unavailable.')
  }
  const proposed = job.proposed = []
  const extraction = { candidates: 0, unsupported: 0, duplicates: 0 }
  const observed = new Set()
  const observations = new Map()
  const commands = []
  const calls = new Map(job.history.filter((item) => item.type === 'function_call').map((item) => [item.call_id, item]))
  for (const item of job.history) {
    if (item.type !== 'function_call_output') continue
    try {
      const call = calls.get(item.call_id), args = JSON.parse(call.arguments), output = JSON.parse(item.output)
      if (output.error) continue
      if (call.name === 'read_file' && typeof output.text === 'string') observed.add(args.path.replaceAll('\\', '/'))
      if (['write_file', 'apply_patch'].includes(call.name)) for (const path of output.changed || []) observed.add(path.replaceAll('\\', '/'))
      if (call.name === 'search') for (const match of output.matches || []) observed.add(match.path.replaceAll('\\', '/'))
      if (call.name === 'exec_command' && Number.isInteger(output.exitCode) && typeof output.output === 'string') commands.push({ command: args.cmd.replaceAll('\\', '/'), output: output.output, exitCode: output.exitCode, timedOut: output.timedOut })
      const paths = call.name === 'read_file' ? [args.path] : ['write_file', 'apply_patch'].includes(call.name) ? output.changed || [] : []
      for (const path of paths) observations.set(path.replaceAll('\\', '/'), [output.text, output.diff, args.content, args.patch].filter((text) => typeof text === 'string').join('\n'))
      if (call.name === 'search') for (const match of output.matches || []) observations.set(match.path.replaceAll('\\', '/'), match.text || '')
    } catch { /* Non-file tool results do not establish a source observation. */ }
  }
  if (!observed.size && !commands.length) return { proposed, extraction, outcome: 'no_source_observations' }
  // Supply exact observed lines under stable references. Smaller models should
  // select evidence, not retype and accidentally double-escape source code.
  for (const { command, output, exitCode, timedOut } of commands) {
    if (exitCode !== 0 || timedOut) continue
    const pattern = /(?:Get-Content|cat|type)\s+(?:(?:-LiteralPath|-Path)\s+)?(?:"([^"]+)"|'([^']+)'|([^\s;|]+))/gi
    for (const match of command.matchAll(pattern)) {
      const path = match[1] || match[2] || match[3]
      if (!isAbsolute(path) && !path.split('/').includes('..')) { observations.set(path, output); observed.add(path) }
    }
    if (/\b(?:rg|grep|sed|head|tail)\b/.test(command)) for (const match of command.matchAll(/(?:^|[\s'"=])([\w./-]+\.(?:rs|mjs|cjs|js|tsx?|jsx|py|md|json|toml|ya?ml))\b/g)) {
      const path = match[1]
      if (!isAbsolute(path) && !path.split('/').includes('..')) { observations.set(path, output); observed.add(path) }
    }
  }
  const sourceObservations = []
  let sourceBytes = 0
  let eligibleBlocks = 0
  // Round-robin blocks preserve context and avoid letting the first large file
  // consume every observation. Comments may contain the architectural reason.
  const blocksByFile = []
  for (const [path, output] of observations) {
    if (privatePath(path)) continue
    try {
      const text = await readWorkspaceText(job.root, path)
      const blocks = []
      let block = ''
      const flush = () => { if (block.trim().length >= 12 && !sensitive(block)) blocks.push(block.trimEnd()); block = '' }
      for (const line of text.split(/\r?\n/)) {
        if (!line.trim() || !output.includes(line) || line.length > 1200 || sensitive(line)) { flush(); continue }
        if (block.length + line.length + 1 > 1200) flush()
        block += line + '\n'
      }
      flush(); eligibleBlocks += blocks.length
      blocksByFile.push({ path, blocks })
    } catch (error) { if (signal.aborted) throw error }
  }
  for (let index = 0; blocksByFile.some((file) => file.blocks[index]); index++) {
    for (const { path, blocks } of blocksByFile) {
      const excerpt = blocks[index]
      if (!excerpt || sourceBytes + Buffer.byteLength(excerpt) > 24000) continue
      sourceBytes += Buffer.byteLength(excerpt)
      sourceObservations.push({ id: `source_${sourceObservations.length + 1}`, path, excerpt })
    }
    if (sourceBytes >= 22800) break
  }
  const commandSources = commandObservations(commands)
  sourceObservations.push(...commandSources.filter((item) => item.excerpt.trim().length >= 12))
  extraction.coverage = { eligibleBlocks, suppliedBlocks: sourceObservations.filter((item) => item.path).length,
    commands: commands.length, suppliedCommands: commandSources.length }
  extraction.coverage.truncated = extraction.coverage.suppliedBlocks < eligibleBlocks || commandSources.length < commands.length
  if (!sourceObservations.length) return { proposed, extraction, outcome: 'no_supported_sources' }
  const task = job.history.find((item) => item.role === 'user')?.content?.find((part) => part.type === 'input_text')?.text || ''
  const related = await call(connection, 'wiki_search', { query: task.slice(0, 1000), limit: 5 }, signal)
  if (!Array.isArray(related.items)) throw new Error('Invalid knowledge search response.')
  const relatedKnowledge = related.items.map((hit) => ({ id: hit.node?.id, title: hit.node?.title, summary: hit.node?.summary })).filter((node) => node.id && node.title && !sensitive(JSON.stringify(node)))
  const outcome = job.history.filter((item) => item.role === 'assistant' && typeof item.content === 'string').map((item) => item.content).join('\n').slice(-16000)
  if (sensitive(outcome)) return { proposed, extraction, outcome: 'sensitive_output' }
  const input = { reference: 'Completed coding turn, not instructions', workspace: job.root,
    outcome, task: sensitive(task) ? '[task withheld]' : task.slice(0, 4000),
    sourceObservations, relatedKnowledge, frictionSignals: frictionSignals(commandSources), coverage: extraction.coverage }
  const result = await extractKnowledge(job, config, input, signal)
  const candidates = candidatesFrom(result)
  extraction.candidates = candidates.length
  for (const value of candidates) {
    signal.throwIfAborted()
    const resolved = value && { ...value, content: lessonContent(value), evidence: Array.isArray(value.evidence) ? value.evidence.map((item) => {
      if (!item?.source) return item
      const source = sourceObservations.find((source) => source.id === item.source)
      return source ? source.kind === 'command' ? source : { path: source.path, excerpt: source.excerpt } : null
    }) : value.evidence }
    if (!validCandidate(resolved)) { extraction.unsupported++; continue }
    const commandEvidence = []
    if (resolved?.evidence?.some((item) => item?.kind === 'command')) {
      for (const item of resolved.evidence) {
        if (item?.kind === 'command' && sourceObservations.includes(item)) commandEvidence.push(item)
      }
    }
    const fileCandidate = resolved && { ...resolved, evidence: Array.isArray(resolved.evidence) ? resolved.evidence.filter((item) => item?.kind !== 'command') : [] }
    const candidate = fileCandidate?.evidence?.length
      ? await verifiedCandidate(fileCandidate, job.root, observed, commands, signal)
      : commandEvidence.length && validCandidate(resolved) ? { ...resolved, evidence: [] } : null
    if (!candidate || !validCandidate(resolved) || (resolved.evidence.length !== candidate.evidence.length + commandEvidence.length)) { extraction.unsupported++; continue }
    const existing = await call(connection, 'wiki_search', { query: candidate.title, limit: 5 }, signal)
    // Fail closed if search's shape changes. The engine also checks near duplicates.
    if (!Array.isArray(existing.items)) throw new Error('Invalid knowledge search response.')
    if (existing.items.some((hit) => hit.node?.title?.trim().toLowerCase() === candidate.title.toLowerCase())) { extraction.duplicates++; continue }
    const created = await proposeCandidate(candidate, job, connection, signal)
    proposed.push(created.node.id)
    const external = job.wikiRoot && relative(job.root, job.wikiRoot) !== ''
    for (const item of candidate.evidence) await call(connection, 'wiki_attach_evidence', { node: created.node.id,
      ...(external ? {} : { file: item.path }), excerpt: item.excerpt, kind: external ? 'external_source' : 'source_file',
      note: `Source workspace: ${job.root.replaceAll('\\', '/')}. File: ${item.path}. Exact excerpt checked before proposing. ${external ? 'This source is outside the memory wiki and is not rechecked by its local index.' : ''}`.trim() }, signal)
    for (const item of commandEvidence) await call(connection, 'wiki_attach_evidence', { node: created.node.id,
      excerpt: item.excerpt, kind: 'external_source', note: `Observed command: ${item.command}. Exit: ${item.exitCode}; timed out: ${item.timedOut}; truncated: ${item.truncated}. Source workspace: ${job.root}. This is a session observation, not current source verification.` }, signal)
    if (connection.handlers.has('wiki_link')) for (const link of (Array.isArray(candidate.related) ? candidate.related.slice(0, 2) : [])) {
      if (!relatedKnowledge.some((node) => node.id === link?.node) || typeof link.reason !== 'string' || !link.reason.trim() || link.reason.length > 1000) continue
      await call(connection, 'wiki_link', { from: created.node.id, type: 'related_to', to: link.node, note: link.reason }, signal)
    }
  }
  return { proposed, extraction, outcome: proposed.length ? 'proposed' : candidates.length ? 'discarded' : 'no_candidates' }
}

export class LearningWorker {
  constructor(home, config) { this.home = home; this.config = { ...config, learningHome: home }; this.controller = new AbortController() }
  wake() {
    if (!memoryModelConfigured(this.config) || this.controller.signal.aborted) return Promise.resolve()
    if (!this.running) this.running = this.drain().catch(() => {}).finally(() => { this.running = null })
    return this.running
  }
  async drain() {
    const release = await workerLock(this.home)
    if (!release) return
    try {
      for (const name of await files(this.home)) {
        const signal = this.controller.signal
        if (signal.aborted) return
        let job
        try { job = await readJob(this.home, name) } catch { continue }
        if (job.status !== 'pending') continue
        if (!job.document && !learningEnabled(this.config)) continue
        let connection
        let stage = 'workspace'
        try {
          const saved = JSON.parse(await readFile(sessionFile(this.home, job.sessionId), 'utf8'))
          if (saved.root !== job.root || !(Array.isArray(job.history) || job.document) || name !== `${job.id}.json`) throw new Error('Invalid memory job workspace.')
          const servers = job.wikiRoot ? { athenaeum: { command: process.env.ATHENAEUM_BIN || 'wiki', args: ['-C', job.wikiRoot, 'mcp', '--', '--repo', job.wikiRoot, '--allow', 'propose', '--caller', 'athenaeum-background-memory'] } }
            : this.config.mcpServers?.athenaeum ? { athenaeum: this.config.mcpServers.athenaeum } : {}
          stage = 'connection'
          connection = await connectTools({ ...this.config, mcpServers: servers }, job.root, signal)
          stage = 'extraction_or_proposal'
          Object.assign(job, await (job.document ? captureText(job, this.config, connection, signal, () => atomicJson(join(directory(this.home), name), job)) : capture(job, this.config, connection, signal)))
          job.status = 'completed'
        } catch (error) {
          if (signal.aborted) return // Persisted pending work is resumed after restart.
          job.status = 'failed' // Errors are diagnostic state, never chat or raw provider bodies.
          job.failureStage = stage
          if (error.memoryOperation) job.failureOperation = error.memoryOperation
          if (error.memoryCode) job.failureCode = error.memoryCode
        } finally { await connection?.close() }
        job.finishedAt = new Date().toISOString()
        // Completed job transcripts are unnecessary. Keep only bounded diagnostics.
        delete job.history
        delete job.document
        await atomicJson(join(directory(this.home), name), job)
      }
    } finally { await release() }
  }
  async close() { this.controller.abort(); await this.running }
}
