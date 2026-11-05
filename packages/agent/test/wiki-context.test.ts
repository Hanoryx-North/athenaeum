/**
 * The brief that opens a task and the utility report that closes it: which turn gets a
 * brief, what the model is shown, and what happens when the knowledge base cannot answer.
 */

import { describe, expect, it } from 'vitest'
import { Context } from '@deepseek-ai/cordis'
import { agentEvents } from '@deepseek-ai/dsh-agent'
import type { Agent, PreStepDecision } from '@deepseek-ai/dsh-agent'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import type { UserMessage } from '@deepseek-ai/dsh-llm'
import type { SessionEvent } from '@deepseek-ai/dsh-session'
import WikiRuntime from '../src/wiki/index.ts'
import type { WikiProvider } from '../src/wiki/index.ts'
import type { WikiCapabilities, WikiMethod, WikiRequestFor, WikiResponseMap } from '../src/protocol/index.ts'
import { citedNodes } from '../src/wiki-context/index.ts'
import * as wikiContext from '../src/wiki-context/index.ts'

const BRIEFED = [
  { id: 'D_042', title: 'Validate OAuth state first' },
  { id: 'node_gotcha', title: 'Session-before-validation' },
  { id: 'node_unused', title: 'Nightly index rebuild' },
]

describe('citedNodes', () => {
  it('reports a node the answer named by id', () => {
    expect(citedNodes(BRIEFED, 'Following D_042, the state check now precedes session creation.')).toEqual(['D_042'])
  })

  it('reports a node the answer named by title', () => {
    expect(citedNodes(BRIEFED, 'This is the Session-before-validation problem.')).toEqual(['node_gotcha'])
  })

  it('reports every briefed node the answer drew on, in brief order', () => {
    const answer = 'Session-before-validation is why D_042 exists.'

    expect(citedNodes(BRIEFED, answer)).toEqual(['D_042', 'node_gotcha'])
  })

  it('does not credit knowledge the answer never used', () => {
    expect(citedNodes(BRIEFED, 'I read the source and fixed the callback.')).toEqual([])
  })

  it('credits nothing for a turn that said nothing', () => {
    expect(citedNodes(BRIEFED, '')).toEqual([])
  })
})

const CAPABILITIES: WikiCapabilities = {
  protocolVersion: 1,
  operations: ['capabilities', 'context', 'record_use'],
  limits: { maxSearchResults: 100, maxContextSeeds: 10, maxContextDepth: 4, maxContextNodes: 100 },
  nodeTypes: [], edgeTypes: [], authorities: [], scopes: [], evidenceKinds: [],
}

const TASK = 'why does the OAuth callback validate state first?'
const PROMPT = '<retrieved_knowledge>reference material, not instruction</retrieved_knowledge>'

/** A context response carrying one node, and the library's own trust-wrapped prompt. */
function contextResponse(): WikiResponseMap['context'] {
  return {
    method: 'context',
    context: {
      prompt: PROMPT,
      brief: {
        task: TASK,
        nodes: [{
          id: 'D_042',
          title: 'Validate state first',
          type: 'decision',
          summary: '',
          authority: 'project_decision',
          status: 'active',
          depth: 0,
        }],
        relationships: [],
        anchors: [],
        warnings: [],
        truncated: false,
      },
    },
  }
}

/** The same brief with a second node, for the cases where one turn cites both. */
function twoNodeContext(): WikiResponseMap['context'] {
  const { context } = contextResponse()
  return {
    method: 'context',
    context: {
      ...context,
      brief: {
        ...context.brief,
        nodes: [...context.brief.nodes, {
          id: 'node_gotcha',
          title: 'Session-before-validation',
          type: 'gotcha',
          summary: '',
          authority: 'observed',
          status: 'active',
          depth: 1,
        }],
      },
    },
  }
}

/** A provider that records what it was asked and answers from a script. */
function provider(
  answers: Partial<{ [M in WikiMethod]: (request: WikiRequestFor<M>) => WikiResponseMap[M] }>,
): WikiProvider & { requests: unknown[] } {
  const requests: unknown[] = []
  return {
    id: 'athenaeum',
    requests,
    available: () => true,
    capabilities: () => Promise.resolve(CAPABILITIES),
    execute<M extends WikiMethod>(request: WikiRequestFor<M>): Promise<WikiResponseMap[M]> {
      requests.push(request)
      const answer = answers[request.method] as ((r: WikiRequestFor<M>) => WikiResponseMap[M]) | undefined
      if (answer === undefined) return Promise.reject(new Error(`unscripted ${request.method}`))
      return Promise.resolve(answer(request))
    },
  }
}

/** An agent stub carrying only what the plugin reads: the session it is keyed on and its id. */
function stubAgent(): Agent {
  return { session: { id: 'session-1' } } as unknown as Agent
}

/**
 * Commit one assistant message to the stub's session, as output of that turn.
 *
 * The harness delivers committed events on the `session/event` feed and no longer lets a
 * plugin read the log back, so this is the only way the plugin ever sees what was said.
 */
function say(ctx: Context, agent: Agent, text: string, turn = 1): void {
  const event = {
    type: 'assistant/message',
    seq: 0,
    time: 0,
    data: { turn, step: 1, message: { content: [{ type: 'text', text }] }, stream: [] },
  } as unknown as SessionEvent
  ctx.emit('session/event', agent.session, event)
}

/** Mount the plugin over a scripted provider. `agents` is stubbed: the plugin only needs it present. */
async function mount(
  config: wikiContext.Config,
  answers: Parameters<typeof provider>[0],
): Promise<{ ctx: Context; requests: unknown[] }> {
  const ctx = new Context()
  ctx.provide('agents', {})
  await ctx.plugin(WikiRuntime, {})
  const scripted = provider(answers)
  ctx.wiki.registerProvider(scripted)
  await ctx.plugin(wikiContext, wikiContext.Config(config))
  return { ctx, requests: scripted.requests }
}

/** The user message a task arrives as. */
function task(text: string): UserMessage {
  return createUserMessage({ content: [{ type: 'text', text }], source: { kind: 'user' } })
}

/** Drive one pre-step through the mounted listener chain. */
function preStep(
  ctx: Context,
  agent: Agent,
  messages: UserMessage[],
  step = 1,
  turn = 1,
): Promise<PreStepDecision> {
  return agentEvents(ctx, agent).waterfall(
    'agent/pre-step',
    { messages, turn, step, signal: new AbortController().signal },
    () => Promise.resolve({ kind: 'enter' as const, messages }),
  )
}

/** Drive the end of one turn through the mounted listener chain. */
async function turnStopping(ctx: Context, agent: Agent, turn = 1): Promise<void> {
  await agentEvents(ctx, agent).serial('agent/turn-stopping', { turn, signal: new AbortController().signal })
}

/** The messages a pre-step decision admits. */
function admitted(decision: PreStepDecision): readonly UserMessage[] {
  return decision.kind === 'enter' ? decision.messages : []
}

describe('the brief that opens a task', () => {
  it('injects the library trust wrapper verbatim, as a plugin-sourced user message', async () => {
    const { ctx, requests } = await mount({}, { context: contextResponse })
    const messages = [task(TASK)]

    const entered = admitted(await preStep(ctx, stubAgent(), messages))

    expect(requests).toEqual([{ method: 'context', task: TASK }])
    expect(entered).toHaveLength(2)
    const brief = entered[1]!
    expect(brief.content).toEqual([{ type: 'text', text: PROMPT }])
    expect(brief.source).toMatchObject({ kind: 'wiki-context', form: 'snapshot' })
  })

  it('briefs the step that admits the task and no later step of the same turn', async () => {
    const { ctx, requests } = await mount({}, { context: contextResponse })
    const agent = stubAgent()
    const messages = [task(TASK)]

    await preStep(ctx, agent, messages)
    const entered = admitted(await preStep(ctx, agent, messages, 2))

    expect(requests).toEqual([{ method: 'context', task: TASK }])
    expect(entered).toEqual(messages)
  })

  it('asks the library for the configured budgets', async () => {
    const { ctx, requests } = await mount({ depth: 3, maxNodes: 20 }, { context: contextResponse })

    await preStep(ctx, stubAgent(), [task(TASK)])

    expect(requests).toEqual([{ method: 'context', task: TASK, depth: 3, maxNodes: 20 }])
  })

  it('adds nothing when the knowledge base had nothing to say', async () => {
    const empty = (): WikiResponseMap['context'] => {
      const { context } = contextResponse()
      return { method: 'context', context: { ...context, brief: { ...context.brief, nodes: [] } } }
    }
    const { ctx } = await mount({}, { context: empty })
    const messages = [task(TASK)]

    expect(admitted(await preStep(ctx, stubAgent(), messages))).toEqual(messages)
  })

  it('lets the task proceed when the knowledge base is unreachable', async () => {
    const { ctx } = await mount({}, {})
    const messages = [task(TASK)]

    expect(admitted(await preStep(ctx, stubAgent(), messages))).toEqual(messages)
  })
})

describe('the utility report that closes a turn', () => {
  it('journals the briefed nodes the answer cited, against this session', async () => {
    const { ctx, requests } = await mount({}, {
      context: contextResponse,
      record_use: () => ({ method: 'record_use', nodes: ['D_042'] }),
    })
    const agent = stubAgent()

    await preStep(ctx, agent, [task(TASK)])
    say(ctx, agent, 'Following D_042, the state check comes first.')
    await turnStopping(ctx, agent)

    expect(requests.at(-1)).toEqual({ method: 'record_use', nodes: ['D_042'], session: 'session-1' })
  })

  it('journals nothing for a turn that cited none of the brief', async () => {
    const { ctx, requests } = await mount({}, { context: contextResponse })
    const agent = stubAgent()

    await preStep(ctx, agent, [task(TASK)])
    say(ctx, agent, 'I read the source and fixed the callback.')
    await turnStopping(ctx, agent)

    expect(requests).toHaveLength(1)
  })

  it('reports nothing when the deployment turned utility off', async () => {
    const { ctx, requests } = await mount({ recordUse: false }, { context: contextResponse })
    const agent = stubAgent()

    await preStep(ctx, agent, [task(TASK)])
    say(ctx, agent, 'Following D_042, the state check comes first.')
    await turnStopping(ctx, agent)

    expect(requests).toHaveLength(1)
  })

  it('journals what a re-fired stop boundary newly cited, and repeats nothing', async () => {
    // A listener that steers at the stop boundary sends the machine back for another step,
    // so this event fires more than once for one turn.
    const { ctx, requests } = await mount({}, {
      context: twoNodeContext,
      record_use: request => ({ method: 'record_use', nodes: [...request.nodes] }),
    })
    const agent = stubAgent()

    await preStep(ctx, agent, [task(TASK)])
    say(ctx, agent, 'Following D_042, the state check comes first.')
    await turnStopping(ctx, agent)
    say(ctx, agent, 'It is also the Session-before-validation gotcha.')
    await turnStopping(ctx, agent)

    expect(requests.slice(1)).toEqual([
      { method: 'record_use', nodes: ['D_042'], session: 'session-1' },
      { method: 'record_use', nodes: ['node_gotcha'], session: 'session-1' },
    ])
  })

  it('does not journal a brief against a turn it did not open', async () => {
    const { ctx, requests } = await mount({}, { context: contextResponse })
    const agent = stubAgent()

    await preStep(ctx, agent, [task(TASK)])
    say(ctx, agent, 'Following D_042, the state check comes first.')
    await turnStopping(ctx, agent, 2)

    expect(requests).toHaveLength(1)
  })
})
