/**
 * One place where a knowledge failure becomes something a model can act on.
 *
 * The protocol distinguishes failures by who is responsible, and that distinction is exactly
 * what the model needs to hear:
 *
 * - the library said "no" (`WikiRemoteError`) — knowledge, or its absence, is the answer;
 * - the library could not be reached (`WikiTransportError`) — the knowledge layer is
 *   unavailable and the task continues without it;
 * - the seam could not choose a provider (`WikiError`) — this deployment has no knowledge
 *   layer installed.
 *
 * Every failure is re-raised as a {@link HarnessError} subclass carrying the original stable
 * code, because that is the only shape the harness records as a structured tool error
 * (`{ name, code }`) rather than an opaque message. A caller that routes on codes keeps
 * working; the model gets a sentence that says what to do instead.
 *
 * @module athenaeum-agent/tool-wiki/errors
 */

import type { Context } from '@deepseek-ai/cordis'
import { HarnessError } from '@deepseek-ai/dsh-llm'
import { WikiRemoteError, WikiTransportError } from '../protocol/index.ts'
import type { WikiMethod, WikiRequestFor, WikiResponseMap } from '../protocol/index.ts'
import { WikiError } from '../wiki/index.ts'
import type {} from '../wiki/index.ts'

/** A knowledge failure as a model-facing tool error. `code` is the original stable code. */
export class WikiToolError extends HarnessError {}

/**
 * Turn any knowledge failure into a structured tool error.
 *
 * @param error - whatever the source threw.
 * @param operation - the operation that failed, as the model knows it.
 * @returns the error to throw; never returns normally.
 */
export function knowledgeFailure(error: unknown, operation: string): WikiToolError {
  if (error instanceof WikiRemoteError) {
    if (error.isWorkspaceMissing) {
      return new WikiToolError(
        'This project has no knowledge base, so there is nothing recorded to answer this. '
        + 'Read the repository directly; the wiki_* tools cannot help until someone initializes one.',
        error.code,
        { cause: error },
      )
    }
    // A near-duplicate rejection names the nodes that already hold the knowledge. Repeating
    // those ids is the difference between the model retrying the same title and attaching
    // what it found to the node that exists.
    const existing = error.existingNodes()
    if (existing.length > 0) {
      return new WikiToolError(
        `${error.message}. Use one of these existing nodes instead: ${existing.join(', ')} — `
        + 'attach what you found to one of them with wiki_attach_evidence.',
        error.code,
        { cause: error },
      )
    }
    return new WikiToolError(`${operation} was refused by the knowledge base: ${error.message}`, error.code, { cause: error })
  }
  if (error instanceof WikiTransportError) {
    return new WikiToolError(
      `The knowledge base could not be reached for ${operation} (${error.code}): ${error.message}. `
      + 'Continue without it: the repository is the source of truth, and this is not a reason to stop.',
      error.code,
      { cause: error },
    )
  }
  if (error instanceof WikiError) {
    return new WikiToolError(
      `No knowledge layer is available for ${operation} (${error.code}): ${error.message}`,
      error.code,
      { cause: error },
    )
  }
  if (error instanceof Error) {
    return new WikiToolError(`${operation} failed: ${error.message}`, 'WIKI_TOOL_FAILURE', { cause: error })
  }
  return new WikiToolError(`${operation} failed: ${String(error)}`, 'WIKI_TOOL_FAILURE')
}

/**
 * Run one knowledge operation and translate its failure for the model.
 *
 * @param ctx - plugin context; `ctx.wiki` is the seam.
 * @param request - the protocol operation.
 * @param signal - cooperative cancellation from the tool execution.
 * @returns the response payload for that method.
 * @throws {WikiToolError} for every failure, carrying the original stable code.
 */
export async function knowledgeCall<M extends WikiMethod>(
  ctx: Context,
  request: WikiRequestFor<M>,
  signal?: AbortSignal,
): Promise<WikiResponseMap[M]> {
  try {
    return await ctx.wiki.execute(request, signal)
  } catch (error: unknown) {
    throw knowledgeFailure(error, request.method)
  }
}
