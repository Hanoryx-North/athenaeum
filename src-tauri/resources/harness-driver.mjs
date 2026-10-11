#!/usr/bin/env node
// Athenaeum's desktop adapter for the documented DeepSeek Harness SDK client.
// stdin/stdout carry this app's small control protocol; the SDK owns the separate
// stdio JSON-RPC connection to the Harness runtime.

import { createInterface } from 'node:readline'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'

const [sdkClientPath, dshHome, profile = 'athenaeum', dshBin] = process.argv.slice(2)
if (!sdkClientPath || !dshHome || !dshBin) {
  process.stderr.write('Harness driver needs the installed SDK client, DSH_HOME, and Harness CLI paths.\n')
  process.exit(2)
}

let DeepSeekHarness
try {
  ;({ DeepSeekHarness } = await import(pathToFileURL(resolve(sdkClientPath)).href))
} catch (error) {
  process.stderr.write(`Could not load the installed DeepSeek Harness SDK client: ${errorMessage(error)}\n`)
  process.exit(2)
}

const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
const send = (message) => process.stdout.write(`${JSON.stringify(message)}\n`)
const defaultProvider = process.env.ATHENAEUM_AGENT_PROVIDER || 'deepseek-official'
const defaultModel = process.env.ATHENAEUM_AGENT_MODEL || 'deepseek-v4-flash'
let harness = null
let workspace = null
let activeRun = null
let queue = Promise.resolve()

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error)
}

async function ensureHarness(projectPath) {
  const cwd = resolve(projectPath)
  if (harness && workspace !== cwd) {
    if (activeRun) throw new Error('A turn is running in another workspace. Stop it before switching projects.')
    await harness.close()
    harness = null
  }
  if (!harness) {
    harness = new DeepSeekHarness({
      profile,
      dshHome,
      dshBin,
      cwd,
      processCwd: cwd,
      provider: defaultProvider,
      model: defaultModel,
    })
    workspace = cwd
  }
  await harness.start()
  return harness
}

async function run(request) {
  if (activeRun) {
    send({ kind: 'error', requestId: request.requestId, message: 'An agent turn is already running.' })
    return
  }
  if (typeof request.projectPath !== 'string' || request.projectPath.length === 0) {
    send({ kind: 'error', requestId: request.requestId, message: 'Open a project before starting an agent turn.' })
    return
  }
  if (typeof request.sessionId !== 'string' || request.sessionId.length === 0) {
    send({ kind: 'error', requestId: request.requestId, message: 'The chat session has no id.' })
    return
  }

  const runState = { requestId: request.requestId, runId: request.runId, cancelled: false }
  activeRun = runState
  try {
    const runtime = await ensureHarness(request.projectPath)
    const contentBlocks = Array.isArray(request.contentBlocks)
      ? request.contentBlocks
      : [{ type: 'text', text: typeof request.message === 'string' ? request.message : '' }]
    const result = await runtime.run(contentBlocks, {
      sessionId: request.sessionId,
      onNotification: (notification) => send({
        kind: 'notification',
        requestId: runState.requestId,
        runId: runState.runId,
        notification,
      }),
    })
    if (runState.cancelled) {
      send({ kind: 'cancelled', requestId: runState.requestId, runId: runState.runId })
    } else {
      send({
        kind: 'result',
        requestId: runState.requestId,
        runId: runState.runId,
        result: { sessionId: result.sessionId, finalResponse: result.finalResponse },
      })
    }
  } catch (error) {
    send(runState.cancelled
      ? { kind: 'cancelled', requestId: runState.requestId, runId: runState.runId }
      : { kind: 'error', requestId: runState.requestId, runId: runState.runId, message: errorMessage(error) })
  } finally {
    activeRun = null
  }
}

async function cancel(request) {
  const current = activeRun
  if (!current || current.runId !== request.runId) {
    send({ kind: 'cancel-accepted', requestId: request.requestId, running: false })
    return
  }
  current.cancelled = true
  // The SDK protocol has no per-turn cancel method. Closing the runtime is its
  // documented cancellation path; ensureHarness creates a fresh runtime later.
  const currentHarness = harness
  harness = null
  workspace = null
  if (currentHarness) {
    try {
      await currentHarness.close()
    } catch (error) {
      process.stderr.write(`Harness shutdown after cancellation: ${errorMessage(error)}\n`)
    }
  }
  send({ kind: 'cancel-accepted', requestId: request.requestId, running: true })
}

async function handle(request) {
  if (!request || typeof request !== 'object' || typeof request.requestId !== 'string') return
  if (request.type === 'cancel') {
    void cancel(request)
    return
  }
  if (request.type === 'shutdown') {
    if (harness) {
      try {
        await harness.close()
      } catch (error) {
        process.stderr.write(`Harness shutdown: ${errorMessage(error)}\n`)
      }
      harness = null
    }
    send({ kind: 'shutdown', requestId: request.requestId })
    input.close()
    return
  }
  if (request.type === 'check') {
    try {
      await ensureHarness(request.projectPath)
      send({ kind: 'ready', requestId: request.requestId, workspace: workspace, provider: defaultProvider, model: defaultModel })
    } catch (error) {
      send({ kind: 'error', requestId: request.requestId, message: errorMessage(error) })
    }
    return
  }
  if (request.type === 'run') await run(request)
}

input.on('line', (line) => {
  let request
  try {
    request = JSON.parse(line)
  } catch {
    return
  }
  if (request?.type === 'cancel') {
    void handle(request)
    return
  }
  queue = queue.then(() => handle(request)).catch((error) => {
    send({ kind: 'error', requestId: request?.requestId, message: errorMessage(error) })
  })
})

input.on('close', () => {
  if (harness) void harness.close().catch((error) => {
    process.stderr.write(`Harness shutdown after driver input closed: ${errorMessage(error)}\n`)
  })
})
