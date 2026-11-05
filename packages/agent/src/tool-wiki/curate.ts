/**
 * The model-facing knowledge curation tools.
 *
 * Curation is deliberately conservative on both sides of the boundary. The library gives a
 * model-created node unverified authority and leaves a model-created relationship proposed
 * and untraversed, whatever this runtime asks for; these tools carry the model identity that
 * makes that attribution possible and say plainly in their descriptions what a claim costs.
 *
 * They are off unless a deployment turns them on. A coding agent that writes to the
 * knowledge base on every hunch fills it with noise, and noise is what makes a knowledge
 * base stop being worth reading.
 *
 * @module athenaeum-agent/tool-wiki/curate
 */

import type { Context } from '@deepseek-ai/cordis'
import { defineTool } from '@deepseek-ai/dsh-tools'
import type { ToolRunContext } from '@deepseek-ai/dsh-tools'
import type {} from '@deepseek-ai/dsh-system-prompt'
import type { WikiCaller } from '../protocol/index.ts'
import type {} from '../wiki/index.ts'
import { knowledgeCall } from './errors.ts'
import { proposal, renderProposals } from './project.ts'

/**
 * Identity for one curation call.
 *
 * The session id is this runtime's opaque handle and the only thing about a conversation
 * that crosses into the knowledge base. Agent messages and tool history stay here.
 *
 * @param model - the configured curator model name.
 * @param exec - the tool execution, whose agent carries the session.
 * @returns the caller identity to attribute the proposal to.
 */
function caller(model: string, exec: ToolRunContext): WikiCaller {
  const session = exec.agent?.session.id
  return { model, ...session === undefined ? {} : { session } }
}

/**
 * Register the curation tools on `ctx.tools`.
 *
 * @param ctx - plugin context; registrations dispose with it.
 * @param model - curator model name attributed to every proposal.
 * @param timeoutMs - cooperative per-call timeout budget attached to each tool.
 */
export function applyCurationTools(ctx: Context, model: string, timeoutMs: number): void {
  ctx.systemPrompt.section({
    name: 'tool:wiki-curation',
    order: 116,
    text: 'You can propose additions to this project\'s knowledge base with wiki_create, wiki_link and '
      + 'wiki_attach_evidence, and inspect what is already waiting with wiki_proposals. '
      + 'Record what a future session would otherwise have to rediscover: a decision and why it was made, a rule '
      + 'the project follows, a gotcha that cost you time. Do not record what the code already says plainly, and '
      + 'do not record a guess. What you add starts unverified, is attributed to you and to this session, and is '
      + 'not knowledge: nothing here accepts a proposal, and only a person reviewing it in Athenaeum can. '
      + 'Attach source evidence, then leave it for that review.',
  })

  ctx.tools.register(defineTool({
    name: 'wiki_create',
    description: 'Record a new piece of project knowledge. Use it for something a future session would otherwise rediscover — a decision, a rule, a gotcha. It is stored as an unverified observation attributed to you until something confirms it.',
    parameters: {
      type: { type: 'string', required: true, description: 'Knowledge type, e.g. decision, rule, gotcha, observation, concept, fact, procedure.' },
      title: { type: 'string', required: true, description: 'Short and specific; this is what future searches match.' },
      summary: { type: 'string', required: true, description: 'One or two sentences stating the claim.' },
      content: { type: 'string', description: 'The full explanation, in markdown.' },
      scope: { type: 'string', description: 'How widely the claim holds, e.g. project, repository, file, technology.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          id: { type: 'string', required: true },
          title: { type: 'string', required: true },
          type: { type: 'string', required: true },
          authority: { type: 'string', required: true },
        },
      },
      render: (_args, value) => [{
        type: 'text',
        text: `Recorded ${value.title} [${value.id}] as ${value.type} with ${value.authority} authority.`,
      }],
    },
    timeoutMs,
    async execute(args, exec) {
      const response = await knowledgeCall(ctx, {
        method: 'create',
        input: {
          type: args.type,
          title: args.title,
          summary: args.summary,
          ...args.content === undefined ? {} : { content: args.content },
          ...args.scope === undefined ? {} : { scope: args.scope },
        },
        caller: caller(model, exec),
      }, exec.signal)
      return {
        id: response.node.id,
        title: response.node.title,
        type: response.node.type,
        authority: response.node.authority,
      }
    },
    presentCall: args => ({ card: 'generic', title: args.title, kind: 'edit', rawInput: args.title }),
  }))

  ctx.tools.register(defineTool({
    name: 'wiki_link',
    description: 'Propose a relationship between two knowledge nodes. Relationships are what make the knowledge base navigable — a decision linked to the gotcha it causes is reachable from either side. Your proposal is stored for review and is not traversed until promoted. At least one end must be a proposal you have not accepted yet; a relationship between two accepted pages is made by editing their Markdown.',
    parameters: {
      from: { type: 'string', required: true, description: 'Source node id or exact title.' },
      type: { type: 'string', required: true, description: 'Relationship type, e.g. depends_on, constrained_by, has_gotcha, supersedes, verified_by.' },
      to: { type: 'string', required: true, description: 'Target node id or exact title.' },
      note: { type: 'string', description: 'One line on why this relationship holds.' },
      confidence: { type: 'number', description: 'How sure you are, from 0 to 1.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          id: { type: 'string', required: true },
          from: { type: 'string', required: true },
          to: { type: 'string', required: true },
          type: { type: 'string', required: true },
          status: { type: 'string', required: true },
        },
      },
      render: (_args, value) => [{
        type: 'text',
        text: `Proposed ${value.from} -${value.type}-> ${value.to} [${value.id}], status ${value.status}.`,
      }],
    },
    timeoutMs,
    async execute(args, exec) {
      const response = await knowledgeCall(ctx, {
        method: 'link',
        input: {
          from: args.from,
          type: args.type,
          to: args.to,
          ...args.note === undefined ? {} : { note: args.note },
          ...args.confidence === undefined ? {} : { confidence: args.confidence },
        },
        caller: caller(model, exec),
      }, exec.signal)
      return {
        id: response.edge.id,
        from: response.edge.from,
        to: response.edge.to,
        type: response.edge.type,
        status: response.edge.status,
      }
    },
    presentCall: args => ({
      card: 'generic',
      title: `${args.from} -${args.type}-> ${args.to}`,
      kind: 'edit',
      rawInput: `${args.from} ${args.type} ${args.to}`,
    }),
  }))

  ctx.tools.register(defineTool({
    name: 'wiki_attach_evidence',
    description: 'Attach the source that supports a knowledge node: a file, a symbol, a test, a commit. Evidence is what lets a later session check a claim instead of trusting it, and what makes staleness detectable when the source moves. It attaches to a proposal that has not been accepted yet; an accepted page cites its sources in its own Markdown.',
    parameters: {
      node: { type: 'string', required: true, description: 'Id or exact title of the proposal the evidence supports.' },
      kind: { type: 'string', description: 'Evidence kind, e.g. source_file, symbol, test, git_commit, documentation.' },
      file: { type: 'string', required: true, description: 'Existing repository-relative evidence file, with forward slashes.' },
      symbol: { type: 'string', description: 'Fully qualified symbol where possible, e.g. OAuthCallback.handle.' },
      line_start: { type: 'number', description: 'First line of the anchored region.' },
      line_end: { type: 'number', description: 'Last line of the anchored region.' },
      excerpt: { type: 'string', description: 'Short quotation from the source.' },
      note: { type: 'string', description: 'What this evidence establishes.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          id: { type: 'string', required: true },
          node: { type: 'string', required: true },
          kind: { type: 'string', required: true },
        },
      },
      render: (_args, value) => [{ type: 'text', text: `Attached ${value.kind} evidence [${value.id}] to ${value.node}.` }],
    },
    timeoutMs,
    async execute(args, exec) {
      const anchor = args.file === undefined
        ? undefined
        : {
          file: args.file,
          ...args.symbol === undefined ? {} : { symbol: args.symbol },
          ...args.line_start === undefined ? {} : { lineStart: args.line_start },
          ...args.line_end === undefined ? {} : { lineEnd: args.line_end },
        }
      const response = await knowledgeCall(ctx, {
        method: 'attach_evidence',
        input: {
          node: args.node,
          ...args.kind === undefined ? {} : { kind: args.kind },
          ...anchor === undefined ? {} : { anchor },
          ...args.excerpt === undefined ? {} : { excerpt: args.excerpt },
          ...args.note === undefined ? {} : { note: args.note },
        },
        caller: caller(model, exec),
      }, exec.signal)
      return { id: response.evidence.id, node: response.evidence.node, kind: response.evidence.kind }
    },
    presentCall: args => ({ card: 'generic', title: args.node, kind: 'edit', rawInput: args.node }),
  }))

  ctx.tools.register(defineTool({
    name: 'wiki_proposals',
    description: 'List the knowledge proposed here that a person has not accepted yet. Check this before proposing something: an existing proposal is better extended with evidence than duplicated. Nothing in this list is knowledge yet, and the agent cannot accept it — only a person reviewing it can.',
    parameters: {
      limit: { type: 'number', description: 'Maximum proposals to return. The library caps this to the limit it advertises.' },
    },
    output: {
      schema: {
        type: 'object',
        additionalProperties: false,
        properties: {
          proposals: {
            type: 'array',
            required: true,
            items: {
              type: 'object',
              additionalProperties: false,
              properties: {
                id: { type: 'string', required: true },
                title: { type: 'string', required: true },
                type: { type: 'string', required: true },
                summary: { type: 'string', required: true },
                authority: { type: 'string', required: true },
                status: { type: 'string', required: true },
                evidence: { type: 'number', required: true },
                relationships: {
                  type: 'array',
                  required: true,
                  items: {
                    type: 'object',
                    additionalProperties: false,
                    properties: {
                      id: { type: 'string', required: true },
                      title: { type: 'string', required: true },
                      type: { type: 'string', required: true },
                      edgeType: { type: 'string', required: true },
                      outbound: { type: 'boolean', required: true },
                      established: { type: 'boolean', required: true },
                      note: { type: 'string' },
                      confidence: { type: 'number' },
                    },
                  },
                },
              },
            },
          },
        },
      },
      render: (_args, value) => [{ type: 'text', text: renderProposals(value.proposals) }],
    },
    timeoutMs,
    isConcurrencySafe: () => true,
    async execute(args, exec) {
      const response = await knowledgeCall(ctx, {
        method: 'proposals',
        ...args.limit === undefined ? {} : { limit: args.limit },
      }, exec.signal)
      return { proposals: response.proposals.map(proposal) }
    },
    presentCall: () => ({ card: 'generic', title: 'proposals awaiting review', kind: 'read', rawInput: 'proposals' }),
  }))
}
