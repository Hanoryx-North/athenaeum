/**
 * Exercise the desktop's shipped Harness driver against the real Harness SDK,
 * the installed Athenaeum Agent profile, the scripted local model and the real
 * Athenaeum engine. No agent loop or bridge behavior is reimplemented here.
 */
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { after, before, describe, it } from 'node:test'

const ROOT = process.env.ATHENAEUM_REPO
const ENGINE = process.env.ATHENAEUM_BIN
const AGENT = process.env.ATHENAEUM_AGENT_ROOT
const HARNESS = process.env.DSH_REPO
const REQUIREMENTS = { ATHENAEUM_REPO: ROOT, ATHENAEUM_BIN: ENGINE, ATHENAEUM_AGENT_ROOT: AGENT, DSH_REPO: HARNESS }
const missing = Object.entries(REQUIREMENTS).filter(([, value]) => !value).map(([key]) => key)
if (missing.length) throw new Error(`desktop Harness validation needs ${missing.join(', ')}`)

const LAUNCHER = join(HARNESS, 'apps', 'cli', 'lib', 'bin.js')
const MOCK_SERVER = join(HARNESS, 'packages', 'test-support', 'llm-mock-server', 'lib', 'index.js')
const DRIVER = join(ROOT, 'src-tauri', 'resources', 'harness-driver.mjs')
const FIXTURE_PAGE = 'wiki/decisions/desktop-session.md'
const PROPOSAL_TITLE = 'State validation precedes session creation'
const PROPOSAL_SUMMARY = 'Validate the login state before creating a session to prevent login CSRF.'

let temporary = ''
let dshHome = ''
let project = ''
let startMockLlmServer

function runEngine(...args) {
  return execFileSync(ENGINE, ['-C', project, ...args], { encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 })
}

function runDsh(args, env) {
  return execFileSync(process.execPath, [LAUNCHER, ...args], {
    cwd: project,
    env: { ...process.env, DSH_HOME: dshHome, ...env },
    encoding: 'utf8',
    maxBuffer: 8 * 1024 * 1024,
  })
}

function createDriver(env) {
  const sdkClient = join(dshHome, 'profiles', 'athenaeum', 'node_modules', '@deepseek-ai', 'dsh-sdk-client', 'lib', 'index.js')
  assert.ok(existsSync(sdkClient), `profile SDK client missing: ${sdkClient}`)
  const child = spawn(process.execPath, [DRIVER, sdkClient, dshHome, 'athenaeum', LAUNCHER], {
    cwd: project,
    env: { ...process.env, DSH_HOME: dshHome, ...env },
    stdio: ['pipe', 'pipe', 'pipe'],
  })
  const messages = []
  const waiters = []
  let stdout = ''
  let stderr = ''
  let closed = false

  child.stdout.on('data', chunk => {
    stdout += String(chunk)
    while (stdout.includes('\n')) {
      const index = stdout.indexOf('\n')
      const line = stdout.slice(0, index)
      stdout = stdout.slice(index + 1)
      if (!line) continue
      let message
      try { message = JSON.parse(line) } catch { continue }
      messages.push(message)
      for (let i = waiters.length - 1; i >= 0; i -= 1) {
        if (waiters[i].matches(message)) {
          const [waiter] = waiters.splice(i, 1)
          clearTimeout(waiter.timer)
          waiter.resolve(message)
        }
      }
    }
  })
  child.stderr.on('data', chunk => { stderr += String(chunk) })
  child.on('close', () => {
    closed = true
    for (const waiter of waiters.splice(0)) {
      clearTimeout(waiter.timer)
      waiter.reject(new Error(`Harness driver closed before replying. ${stderr}`))
    }
  })

  function waitFor(matches, label, timeoutMs = 120_000) {
    const existing = messages.find(matches)
    if (existing) return Promise.resolve(existing)
    if (closed) return Promise.reject(new Error(`Harness driver already closed while waiting for ${label}. ${stderr}`))
    return new Promise((resolve, reject) => {
      const waiter = { matches, resolve, reject, timer: undefined }
      waiter.timer = setTimeout(() => {
        const index = waiters.indexOf(waiter)
        if (index >= 0) waiters.splice(index, 1)
        reject(new Error(`Timed out waiting for ${label}. ${stderr}`))
      }, timeoutMs)
      waiters.push(waiter)
    })
  }

  async function request(message, timeoutMs) {
    const response = waitFor(
      item => item.kind !== 'notification' && item.requestId === message.requestId,
      `${message.type} response ${message.requestId}`,
      timeoutMs,
    )
    child.stdin.write(`${JSON.stringify(message)}\n`)
    return response
  }

  return { child, messages, request, stderr: () => stderr }
}

describe('desktop Harness driver → SDK → Agent profile → local engine', () => {
  before(async () => {
    temporary = mkdtempSync(join(tmpdir(), 'athenaeum desktop chat-'))
    dshHome = join(temporary, 'dsh-home')
    project = join(temporary, 'workspace')
    mkdirSync(project, { recursive: true })
    project = realpathSync(project)

    runEngine('init')
    mkdirSync(dirname(join(project, FIXTURE_PAGE)), { recursive: true })
    writeFileSync(join(project, FIXTURE_PAGE), [
      '---',
      'type: decision',
      'title: Desktop session boundary',
      'authority: project_decision',
      '---',
      '',
      '# Desktop session boundary',
      '',
      'Validate login state before creating a session.',
      '',
    ].join('\n'), 'utf8')
    runEngine('index-vault')

    runDsh(['--profile', 'athenaeum', '--from-default-profile', 'headless', '--dump-default-config'])
    runDsh(['plugin', '--profile', 'athenaeum', 'add', AGENT])
    const pin = JSON.parse(readFileSync(join(AGENT, 'harness.pin.json'), 'utf8'))
    const clientPackage = JSON.parse(readFileSync(join(HARNESS, 'packages', 'sdk', 'client', 'package.json'), 'utf8'))
    const protocolPackage = JSON.parse(readFileSync(join(HARNESS, 'packages', 'sdk', 'protocol', 'package.json'), 'utf8'))
    assert.equal(clientPackage.version, pin.version, 'the app profile and runtime must use the pinned SDK release')
    runDsh(['plugin', '--profile', 'athenaeum', 'add', `@deepseek-ai/dsh-sdk-jsonrpc-server@${pin.version}`])
    runDsh(['plugin', '--profile', 'athenaeum', 'add', `@deepseek-ai/dsh-sdk-protocol@${protocolPackage.version}`])
    // Match the installer: the client is a desktop dependency, not a profile plugin.
    // Mounting it through the plugin manager installs a second Harness and requests
    // native-build approvals instead of exercising the shipped SDK connection.
    const sdkLink = join(dshHome, 'profiles', 'athenaeum', 'node_modules', '@deepseek-ai', 'dsh-sdk-client')
    mkdirSync(dirname(sdkLink), { recursive: true })
    symlinkSync(join(HARNESS, 'packages', 'sdk', 'client'), sdkLink, process.platform === 'win32' ? 'junction' : 'dir')

    const profilePatch = join(dshHome, 'profiles', 'athenaeum', 'cordis.patch.yml')
    mkdirSync(join(dshHome, 'profiles', 'athenaeum'), { recursive: true })
    copyFileSync(join(AGENT, 'profile', 'cordis.patch.yml'), profilePatch)
    writeFileSync(profilePatch, readFileSync(profilePatch, 'utf8') + [
      '- id: session-log-deepseek',
      '  config:',
      '    enabled: false',
      '- id: wiki-athenaeum',
      '  config:',
      `    command: ${JSON.stringify(ENGINE)}`,
      '',
    ].join('\n'), 'utf8')

    const mock = await import(pathToFileURL(MOCK_SERVER).href)
    startMockLlmServer = mock.startMockLlmServer
  }, 180_000)

  after(async () => {
    if (temporary) rmSync(temporary, { recursive: true, force: true })
  })

  it('uses the selected workspace, reports Harness events and leaves wiki knowledge pending until a person accepts', async () => {
    const model = await startMockLlmServer({
      sequence: ['tool_call_success', 'success'],
      repeatLast: true,
      toolName: 'wiki_create',
      toolArguments: JSON.stringify({
        type: 'decision',
        title: PROPOSAL_TITLE,
        summary: PROPOSAL_SUMMARY,
      }),
      successText: 'I proposed the state validation decision for review.',
    })
    const driver = createDriver({
      DEEPSEEK_BASE_URL: `${model.baseURL}/v1`,
      DEEPSEEK_API_KEY: 'local-validation-key',
      DSH_TELEMETRY_MODE: 'DISABLED',
      NO_PROXY: '127.0.0.1,localhost',
      no_proxy: '127.0.0.1,localhost',
    })

    try {
      const status = await driver.request({
        type: 'check',
        requestId: 'desktop-check',
        projectPath: project,
      }, 30_000)
      assert.equal(status.kind, 'ready', JSON.stringify(status))
      assert.equal(status.workspace, project)
      assert.equal(status.provider, 'deepseek-official')
      assert.equal(status.model, 'deepseek-v4-flash')

      const runId = 'desktop-proposal-run'
      const result = await driver.request({
        type: 'run',
        requestId: runId,
        runId,
        projectPath: project,
        sessionId: 'desktop-proposal-session',
        contentBlocks: [{ type: 'text', text: 'Record the decision we learned as a wiki proposal.' }],
      }, 120_000)
      assert.equal(result.kind, 'result', JSON.stringify(result))
      assert.equal(result.result.sessionId, 'desktop-proposal-session')
      assert.match(result.result.finalResponse, /proposed.*review/i)

      const notifications = driver.messages
        .filter(message => message.kind === 'notification' && message.runId === runId)
        .map(message => message.notification)
      assert.ok(notifications.some(notification => notification.method === 'session.status'
        && notification.params.status === 'running'), 'the client should receive Harness status updates')
      assert.ok(notifications.some(notification => notification.method === 'session.event'
        && notification.params.event?.type === 'tool/call'
        && notification.params.event?.data?.name === 'wiki_create'), 'the client should receive the real proposal tool call')
      assert.ok(notifications.some(notification => notification.method === 'session.event'
        && notification.params.event?.type === 'tool/result'
        && notification.params.event?.data?.message?.isError !== true), 'the client should receive the real tool result')

      const proposalOutput = runEngine('--json', 'proposals')
      const proposals = JSON.parse(proposalOutput)
      const proposal = proposals.find(item => item.node?.title === PROPOSAL_TITLE)
      assert.ok(proposal, 'the curation tool should create a proposal in the local engine')
      const wikiRoot = join(project, 'wiki')
      const pagesBeforeReview = existsSync(wikiRoot)
        ? readdirSync(wikiRoot, { recursive: true }).filter(path => String(path).endsWith('.md'))
        : []
      assert.deepEqual(pagesBeforeReview.map(path => String(path).replaceAll('\\', '/')).sort(), [FIXTURE_PAGE.slice('wiki/'.length)],
        'a proposal must not create an accepted Markdown page')

      const accepted = JSON.parse(runEngine('--json', 'accept', proposal.node.id, '--as', 'desktop-validation-reviewer'))
      assert.ok(accepted.page?.path, 'accepting a proposal in Athenaeum should return its canonical page')
      assert.ok(existsSync(join(project, accepted.page.path)), 'the accepted proposal should become a Markdown page')

      let requestSeenResolve
      const requestSeen = new Promise(resolve => { requestSeenResolve = resolve })
      const stalledModel = await startMockLlmServer({
        sequence: ['stall'],
        onEvent: event => { if (event.type === 'request') requestSeenResolve() },
      })
      const stoppedDriver = createDriver({
        DEEPSEEK_BASE_URL: `${stalledModel.baseURL}/v1`,
        DEEPSEEK_API_KEY: 'local-validation-key',
        DSH_TELEMETRY_MODE: 'DISABLED',
        NO_PROXY: '127.0.0.1,localhost',
        no_proxy: '127.0.0.1,localhost',
      })
      try {
        const check = await stoppedDriver.request({
          type: 'check',
          requestId: 'desktop-cancel-check',
          projectPath: project,
        }, 30_000)
        assert.equal(check.kind, 'ready', JSON.stringify(check))
        const pendingRun = stoppedDriver.request({
          type: 'run',
          requestId: 'desktop-cancel-run',
          runId: 'desktop-cancel-run',
          projectPath: project,
          sessionId: 'desktop-cancel-session',
          contentBlocks: [{ type: 'text', text: 'Wait for the current runtime request.' }],
        }, 60_000)
        let requestTimer
        try {
          await Promise.race([
            requestSeen,
            new Promise((_, reject) => { requestTimer = setTimeout(() => reject(new Error('the scripted model did not receive the turn')), 30_000) }),
          ])
        } finally {
          clearTimeout(requestTimer)
        }
        const cancelled = await stoppedDriver.request({
          type: 'cancel',
          requestId: 'desktop-cancel-request',
          runId: 'desktop-cancel-run',
        }, 30_000)
        assert.equal(cancelled.kind, 'cancel-accepted')
        assert.equal((await pendingRun).kind, 'cancelled', 'Stop should close the SDK runtime and settle the active turn')
      } finally {
        try { await stoppedDriver.request({ type: 'shutdown', requestId: 'desktop-cancel-shutdown' }, 10_000) } catch {}
        if (stoppedDriver.child.exitCode === null) stoppedDriver.child.kill('SIGKILL')
        await stalledModel.close()
      }
    } finally {
      try { await driver.request({ type: 'shutdown', requestId: 'desktop-shutdown' }, 10_000) } catch {}
      if (driver.child.exitCode === null) driver.child.kill('SIGKILL')
      await model.close()
    }
  })
})
