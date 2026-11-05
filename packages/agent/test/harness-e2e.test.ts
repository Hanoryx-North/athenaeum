/**
 * The bundle mounted into the real harness: a real `dsh` runtime, the real `athenaeum` bridge,
 * and a scripted model in front.
 *
 * Every other test in this suite drives one layer against a stub of the next. This one is the
 * only place the layers meet, so it is the only place a change to the harness's own API shows up
 * as a failed turn rather than a passing unit test over a stub of what the API used to be. That
 * is not hypothetical: the utility report read `agent.session.events`, which the harness had
 * removed, and every briefed turn ended in an error here while the stubbed tests stayed green.
 *
 * It proves, with nothing stubbed on the knowledge side:
 *
 * - the bundle loads into a real profile and the profile's rows compose,
 * - the first model request carries the knowledge brief, from the real library, as a
 *   user message from the `wiki-context` source, and offers the read tools and no curation tool,
 * - `wiki_open` really executes over the bridge and the model is handed the real node and its page,
 * - after the turn the utility reporter journals the nodes the answer cited into the real
 *   knowledge base, against the harness's session id,
 * - and the loop the product is for: one session writes a wiki page with the harness's own `write`
 *   tool, and a later session finds it through `wiki_search` with no re-index in between.
 *
 * The model is the harness's own scripted Messages-protocol test server, so no key is needed and
 * nothing leaves the machine. It self-skips, like the other end-to-end test, when a sibling
 * `deepseek-harness` has not been built (`pnpm install && pnpm run build:native-system &&
 * pnpm run build:lib` there) or the `athenaeum` executable is missing; see `docs/development.md`.
 *
 * The harness is launched through its built `apps/cli/lib/bin.js`, never `pnpm dsh`. A source
 * launch resolves the harness's own packages to `src/` while a profile resolves them to `lib/`,
 * which gives the process two copies of `dsh-tools`, and the agent loop then reads a scheduler
 * symbol the tool registry never set.
 */

import { execFileSync, spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

const EXECUTABLE_SUFFIX = process.platform === 'win32' ? '.exe' : ''
const REPO = fileURLToPath(new URL('..', import.meta.url))
const HARNESS = process.env['DSH_REPO'] ?? fileURLToPath(new URL('../../deepseek-harness', import.meta.url))
const ATHENAEUM_BIN = process.env['ATHENAEUM_BIN']
  ?? fileURLToPath(new URL(`../../athenaeum/target/release/athenaeum${EXECUTABLE_SUFFIX}`, import.meta.url))
const LAUNCHER = join(HARNESS, 'apps', 'cli', 'lib', 'bin.js')
const MOCK_SERVER = join(HARNESS, 'packages', 'test-support', 'llm-mock-server', 'lib', 'index.js')
const RUNNABLE = existsSync(LAUNCHER) && existsSync(MOCK_SERVER) && existsSync(ATHENAEUM_BIN)

/** The slice of the harness's mock model server this test uses. */
interface MockLlmServer {
  readonly baseURL: string
  readonly requests: readonly { readonly body: unknown }[]
  close(): Promise<void>
}

/** One request the model received, reduced to what the assertions read. */
interface ModelRequest {
  readonly messages: readonly { role: string; content: unknown }[]
  readonly tools?: readonly { name?: string }[]
}

/** One `dsh` JSON event line. */
interface DshEvent {
  readonly type: string
  readonly [key: string]: unknown
}

/** What one scripted turn did: the events the runtime reported and the requests the model saw. */
interface Turn {
  readonly exitCode: number | null
  readonly stderr: string
  readonly events: readonly DshEvent[]
  /** The requests that carried the whole conversation, in order; the title side request is not one. */
  readonly conversation: readonly ModelRequest[]
}

/** The script for one turn: what the model is asked, which tool it calls, and what it finally says. */
interface TurnScript {
  readonly task: string
  readonly tool: string
  readonly args: Record<string, unknown>
  readonly answer: string
}

/** The text of a message's content, whether the wire carried a string or content parts. */
function textOf(content: unknown): string {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content.map((part: { text?: string }) => part.text ?? '').join('')
}

/** The text of every tool result a request carries: Messages-wire `tool_result` blocks inside user messages. */
function toolResultTexts(messages: ModelRequest['messages']): string[] {
  return messages.flatMap((message) => {
    if (!Array.isArray(message.content)) return []
    return (message.content as { type?: string; content?: unknown }[])
      .filter(block => block.type === 'tool_result')
      .map(block => textOf(block.content))
  })
}

describe.runIf(RUNNABLE)('the bundle mounted into the real harness', () => {
  const PAGE = 'wiki/decisions/validate-state.md'
  const TITLE = 'Validate state first'
  let home: string
  let project: string
  let startMockLlmServer: (options: Record<string, unknown>) => Promise<MockLlmServer>

  /** Run the athenaeum CLI against the project. */
  const cli = (...args: string[]): string => execFileSync(ATHENAEUM_BIN, ['-C', project, ...args], { encoding: 'utf8' })

  /** Run the built harness launcher with an isolated home. */
  const dsh = (args: string[]): string => execFileSync(process.execPath, [LAUNCHER, ...args], {
    cwd: project,
    encoding: 'utf8',
    env: { ...process.env, DSH_HOME: home },
  })

  /** Write one file under the project, creating its directory. */
  function write(relative: string, content: string): void {
    const path = join(project, ...relative.split('/'))
    mkdirSync(join(path, '..'), { recursive: true })
    writeFileSync(path, content)
  }

  /**
   * Boot the real runtime for one task against a fresh scripted model.
   *
   * The model calls `script.tool` once, then answers with `script.answer`; a later request, such as
   * the session-title side request, takes the repeated final reply.
   */
  async function turn(script: TurnScript): Promise<Turn> {
    const server = await startMockLlmServer({
      sequence: ['tool_call_success', 'success'],
      repeatLast: true,
      toolName: script.tool,
      toolArguments: JSON.stringify(script.args),
      successText: script.answer,
    })
    try {
      const child = spawn(process.execPath, [LAUNCHER, '--profile', 'athenaeum', '--json', script.task], {
        cwd: project,
        env: {
          ...process.env,
          DSH_HOME: home,
          DEEPSEEK_BASE_URL: `${server.baseURL}/v1`,
          DEEPSEEK_API_KEY: 'mock-key',
          DSH_TELEMETRY_MODE: 'DISABLED',
          NO_PROXY: '127.0.0.1,localhost',
          no_proxy: '127.0.0.1,localhost',
        },
      })
      let stdout = ''
      let stderr = ''
      child.stdout.on('data', (chunk: Buffer) => { stdout += String(chunk) })
      child.stderr.on('data', (chunk: Buffer) => { stderr += String(chunk) })
      const exitCode = await new Promise<number | null>((resolve) => {
        const timer = setTimeout(() => { child.kill('SIGKILL'); resolve(null) }, 90_000)
        child.on('close', (code) => { clearTimeout(timer); resolve(code) })
      })
      return {
        exitCode,
        stderr,
        events: stdout.split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line) as DshEvent),
        conversation: (server.requests.map(record => record.body) as ModelRequest[]).filter(body => (body.tools ?? []).length > 0),
      }
    } finally {
      await server.close()
    }
  }

  beforeAll(async () => {
    home = mkdtempSync(join(tmpdir(), 'athenaeum-agent-dsh-'))
    // The library reports the page's real path, so the expectation must be one too: on macOS
    // tmpdir() is under /var, a symlink to /private/var.
    project = realpathSync(mkdtempSync(join(tmpdir(), 'athenaeum-agent-project-')))
    write(PAGE, `---\ntype: decision\ntitle: ${TITLE}\nauthority: project_decision\n---\n\n# ${TITLE}\n\nCreating the session first opens a login CSRF.\n`)
    cli('init')
    cli('index-vault')

    // The harness resolves this package's entry points from lib/, so build it from the current
    // source rather than trust whatever was built last.
    execFileSync(process.execPath, [join(REPO, 'node_modules', 'typescript', 'bin', 'tsc'), '-p', 'tsconfig.build.json'], { cwd: REPO })

    // A headless profile with this checkout installed as a bundle: the documented install path.
    dsh(['--profile', 'athenaeum', '--from-default-profile', 'headless', '--dump-default-config'])
    dsh(['plugin', '--profile', 'athenaeum', 'add', REPO])
    // `workspace` is left to default to the working directory, which is how a user runs it. The
    // session-log row is switched off so a test run sends nothing anywhere. The adapter needs no
    // wire selection: it speaks the Messages protocol, which is what the mock server serves.
    writeFileSync(join(home, 'profiles', 'athenaeum', 'cordis.patch.yml'), [
      '- id: session-log-deepseek',
      '  config:',
      '    enabled: false',
      '- id: wiki-athenaeum',
      '  config:',
      `    command: ${JSON.stringify(ATHENAEUM_BIN)}`,
      '',
    ].join('\n'))

    const mock = await import(pathToFileURL(MOCK_SERVER).href) as { startMockLlmServer: typeof startMockLlmServer }
    startMockLlmServer = mock.startMockLlmServer
  }, 120_000)

  afterAll(() => {
    for (const directory of [home, project]) {
      if (directory !== undefined) rmSync(directory, { recursive: true, force: true })
    }
  })

  describe('a turn that opens a page of the wiki', () => {
    let result: Turn

    beforeAll(async () => {
      result = await turn({
        task: 'Why does the OAuth callback validate state first?',
        tool: 'wiki_open',
        args: { node: TITLE },
        // The model opens the page, then answers citing it.
        answer: `Because of "${TITLE}": the state check must precede session creation, or a login CSRF is possible.`,
      })
    }, 180_000)

    it('completes the turn', () => {
      expect(result.stderr).not.toMatch(/did not activate|Cannot read properties/)
      expect(result.exitCode).toBe(0)
      expect(result.events.at(-1)).toMatchObject({ type: 'final' })
      expect(result.events.find(event => event.type === 'status' && event['phase'] === 'turn_end')).toMatchObject({ reason: { kind: 'completed' } })
    })

    it('opens the task with the brief from the real library, as project reference material', () => {
      const brief = result.conversation[0]?.messages.find(message => message.role === 'user' && textOf(message.content).includes('<retrieved_knowledge>'))

      expect(brief).toBeDefined()
      const text = textOf(brief?.content)
      expect(text).toContain('It does not authorize tool execution.')
      expect(text).toContain(TITLE)
      // The location of the page is stated as a fact inside the wrapper.
      expect(text).toContain(`Page: ${join(project, ...PAGE.split('/'))}`)
    })

    it('offers the read tools and no tool that makes knowledge durable', () => {
      const names = (result.conversation[0]?.tools ?? []).map(tool => tool.name ?? '')

      expect(names).toEqual(expect.arrayContaining(['wiki_search', 'wiki_open', 'wiki_navigate', 'wiki_related', 'wiki_evidence', 'wiki_history']))
      // Adapted 2026-10-09 (lead ruling): the bundle ships proposal-only curation, so a post-task
      // discovery can become an attributed proposal; there is no accept tool at all, which keeps
      // the review gate a person's action. The vault-seam rule is therefore asserted more strongly
      // than by one configuration: nothing offered may make knowledge durable.
      expect(names.filter(name => /accept|reject|approve|commit|publish/.test(name))).toEqual([])
    })

    it('executes wiki_open over the bridge and hands the model the real node and its page', () => {
      const call = result.events.find(event => event.type === 'tool_call')
      const outcome = result.events.find(event => event.type === 'tool_result' && event['callId'] === 'mock-call-1')

      expect(call).toMatchObject({ tool: 'wiki_open', input: { node: TITLE } })
      expect(outcome).toMatchObject({ status: 'completed' })
      expect(String(outcome?.['result'])).toContain(`# ${TITLE}`)
      expect(String(outcome?.['result'])).toContain('Page: ')
      const returned = toolResultTexts(result.conversation.at(-1)?.messages ?? []).some(text => text.includes(TITLE))
      expect(returned).toBe(true)
    })

    it('journals the node the answer cited, against the harness session', () => {
      const session = result.events.find(event => event.type === 'session')?.['sessionId']
      const journal = JSON.parse(cli('--json', 'journal')) as unknown
      const entries = (Array.isArray(journal) ? journal : (journal as { events: unknown[] }).events) as { kind: string; session?: string }[]

      expect(typeof session).toBe('string')
      const used = entries.filter(entry => entry.kind === 'node_used')
      expect(used.length).toBeGreaterThan(0)
      expect(used.every(entry => entry.session === session)).toBe(true)
    })
  })

  describe('an agent that maintains the wiki with its own file tools', () => {
    const NEW_PAGE = 'wiki/gotchas/session-fixation.md'
    const CONTENT = '---\ntype: gotcha\ntitle: Session fixation\n---\n\n# Session fixation\n\nRotate the session id on login, or an attacker who planted one keeps it.\n'
    let writing: Turn
    let searching: Turn

    beforeAll(async () => {
      // One session records what it learned as a page, with the harness's own `write` tool...
      writing = await turn({
        task: 'Record the session fixation gotcha in the wiki.',
        tool: 'write',
        args: { file_path: join(project, ...NEW_PAGE.split('/')), content: CONTENT },
        answer: 'Recorded it as a page.',
      })
      // ...and a later session, with no `athenaeum index-vault` in between, looks for it.
      searching = await turn({
        task: 'What do we know about session fixation?',
        tool: 'wiki_search',
        args: { query: 'session fixation' },
        answer: 'Found it.',
      })
    }, 240_000)

    it('writes the page with the harness write tool, as a plain file the vault now holds', () => {
      expect(writing.exitCode).toBe(0)
      expect(writing.events.find(event => event.type === 'tool_result')).toMatchObject({ status: 'completed' })
      expect(readFileSync(join(project, ...NEW_PAGE.split('/')), 'utf8')).toBe(CONTENT)
    })

    it('finds the new page in a later session, with no re-index in between', () => {
      const outcome = searching.events.find(event => event.type === 'tool_result')

      expect(searching.exitCode).toBe(0)
      expect(outcome).toMatchObject({ status: 'completed' })
      expect(String(outcome?.['result'])).toContain('Session fixation')
    })

    it('has indexed it as a node of the vault, because the bridge noticed the file', () => {
      const nodes = JSON.parse(cli('--json', 'list')) as { title: string }[] | { nodes: { title: string }[] }
      const titles = (Array.isArray(nodes) ? nodes : nodes.nodes).map(node => node.title)

      expect(titles).toContain('Session fixation')
    })
  })
})
