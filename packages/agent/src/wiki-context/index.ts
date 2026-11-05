/**
 * Consumer plugin: the knowledge brief that opens a task, and the utility report that closes it.
 *
 * The brief is injected as a user message rather than a system-prompt section, for two
 * reasons that both matter. It is specific to one task, so putting it in a cached system
 * prompt would invalidate that cache every turn; and a durable `user/message` event is what
 * makes the brief reconstructable from the session log, which the harness requires of
 * anything a model sees.
 *
 * The injected text is the library's own trust-wrapped prompt, verbatim. It states that
 * retrieved knowledge is project reference material that does not override instructions,
 * grant permissions, or authorize tools. Re-rendering the structured brief here would drop
 * that framing, so this plugin never does.
 *
 * @module athenaeum-agent/wiki-context
 */

import type { Context } from '@deepseek-ai/cordis'
import z from '@deepseek-ai/schemastery'
import type { PreStepDecision } from '@deepseek-ai/dsh-agent'
import { createUserMessage } from '@deepseek-ai/dsh-llm'
import type { ContextFormed, UserMessage } from '@deepseek-ai/dsh-llm'
import type { Session } from '@deepseek-ai/dsh-session'
import type {} from '../wiki/index.ts'

/** Cordis plugin name used by loader diagnostics, and the message source this plugin owns. */
export const name = 'wiki-context'

// The harness has no shared catch-all source kind: each producer declares its own, so the
// session log can tell a knowledge brief from a hook's context or a webhook's payload.
declare module '@deepseek-ai/dsh-llm' {
  interface MessageSourceMap {
    'wiki-context': { kind: 'wiki-context' } & ContextFormed
  }
}

/** Services the brief requires. */
export const inject = ['agents', 'wiki']

/** How much knowledge one brief may carry, and whether utility is reported. */
export interface Config {
  /** Relationship hops to walk from the seed nodes. Omit to use the library's default. */
  depth?: number
  /** Upper bound on nodes in one brief. The library caps this to the limit it advertises. */
  maxNodes?: number
  /**
   * Report which briefed nodes the turn actually cited. Defaults to true.
   *
   * Utility is what later separates knowledge that earns its place from knowledge that keeps
   * surfacing and never helps, so a deployment that turns this off keeps the brief but loses
   * the signal that would prune it.
   */
  recordUse?: boolean
}

export const Config: z<Config> = z.object({
  depth: z.number().step(1).min(1),
  maxNodes: z.number().step(1).min(1),
  recordUse: z.boolean().default(true),
})

/** What one turn briefed, kept until the turn ends and its utility can be reported. */
interface BriefedTurn {
  /** The turn the brief was injected into, as `agent/pre-step` reported it. */
  readonly turn: number
  /** Node ids in the injected brief, paired with the titles a model would cite them by. */
  readonly nodes: readonly { readonly id: string; readonly title: string }[]
  /** Node ids already journaled for this turn, so a second stop boundary cannot double-count them. */
  readonly reported: Set<string>
  /** The assistant text of this turn, appended as each message commits to the session. */
  readonly answer: string[]
}

/** Concatenate the text blocks of the human messages entering this step. */
function taskText(messages: readonly UserMessage[]): string {
  return messages
    .filter(message => message.source.kind === 'user')
    .flatMap(message => message.content.flatMap(block => block.type === 'text' ? [block.text] : []))
    .join('\n')
    .trim()
}

/**
 * Which briefed nodes the turn actually drew on.
 *
 * A node counts as used when the answer names it — by id or by title. Reporting the whole
 * brief instead would record every retrieval as a success and destroy the one signal that
 * distinguishes useful knowledge from noise.
 *
 * @param briefed - the nodes injected at the start of the turn.
 * @param answer - everything the assistant said during it.
 * @returns the ids worth journaling as used.
 */
export function citedNodes(briefed: BriefedTurn['nodes'], answer: string): string[] {
  if (answer.length === 0) return []
  return briefed
    .filter(node => answer.includes(node.id) || (node.title.length > 0 && answer.includes(node.title)))
    .map(node => node.id)
}

/**
 * Register the brief injector and the utility reporter for the lifetime of `ctx`.
 *
 * @param ctx - plugin context; both listeners dispose with it.
 * @param config - retrieval budgets and whether to report utility.
 */
export function apply(ctx: Context, config: Config): void {
  const briefedTurns = new WeakMap<Session, BriefedTurn>()

  ctx.on('agent/pre-step', async ({ agent, step, turn, signal }, next): Promise<PreStepDecision> => {
    const decision = await next()
    // The brief answers one task, so it belongs to the step that admits that task. Later
    // steps in the same turn are the model working through it and need no second copy.
    if (decision.kind === 'reject' || signal.aborted || step !== 1) return decision
    const task = taskText(decision.messages)
    if (task.length === 0) return decision
    let context
    try {
      context = await ctx.wiki.context({
        task,
        ...config.depth === undefined ? {} : { depth: config.depth },
        ...config.maxNodes === undefined ? {} : { maxNodes: config.maxNodes },
      }, signal)
    } catch (error: unknown) {
      // Knowledge is an accelerator: the agent can always read the repository itself, so a
      // knowledge base that is unreachable must never take the task down with it.
      ctx.logger.warn(`${name}: no knowledge brief for this task: ${String(error)}`)
      return decision
    }
    if (context === undefined) return decision
    briefedTurns.set(agent.session, {
      turn,
      nodes: context.brief.nodes.map(node => ({ id: node.id, title: node.title })),
      reported: new Set(),
      answer: [],
    })
    const text = context.prompt
    return {
      kind: 'enter',
      messages: [
        ...decision.messages,
        createUserMessage({
          content: [{ type: 'text', text }],
          source: { kind: 'wiki-context', form: 'snapshot', sections: [{ name, text }] },
        }),
      ],
    }
  }, { prepend: true })

  if (config.recordUse === false) return

  // The harness retires synchronous reads of past session events, so the answer is collected
  // from the feed of committed events instead of read back from the log at the stop boundary.
  ctx.on('session/event', (session, event) => {
    if (event.type !== 'assistant/message') return
    const briefed = briefedTurns.get(session)
    if (briefed === undefined || briefed.turn !== event.data.turn) return
    for (const block of event.data.message.content) {
      if (block.type === 'text') briefed.answer.push(block.text)
    }
  })

  ctx.on('agent/turn-stopping', async ({ agent, turn, signal }): Promise<void> => {
    const briefed = briefedTurns.get(agent.session)
    if (briefed === undefined || briefed.turn !== turn) return
    // The stop boundary is not always the end of the turn: a listener that objects steers,
    // and the machine runs another step and stops again. Journalling the delta each time,
    // rather than dropping the brief on the first pass, is what keeps knowledge the model
    // only cited after the steer from going unrecorded.
    const used = citedNodes(briefed.nodes, briefed.answer.join('\n'))
      .filter(id => !briefed.reported.has(id))
    if (used.length === 0) return
    try {
      await ctx.wiki.recordUse(used, agent.session.id, signal)
      for (const id of used) briefed.reported.add(id)
    } catch (error: unknown) {
      // The work is already done and reported. A journal that cannot be written must not
      // retroactively fail a finished turn.
      ctx.logger.warn(`${name}: could not report knowledge utility: ${String(error)}`)
    }
  })
}
