/**
 * The desktop's one door to the engine's knowledge surfaces.
 *
 * Every call here goes to the `engine_request` Tauri command, which validates
 * the request, forwards it over the same in-process bridge an agent uses, and
 * validates the answer against its own DTOs. This module adds the parts the
 * UI needs around that call:
 *
 * * **De-duplication.** Two panels asking for the same thing at the same time
 *   make one round trip, not two.
 * * **Generations.** Switching project, or explicitly reloading, bumps a
 *   counter. An answer that arrives for an older generation is refused, so a
 *   slow reply can never paint one project's knowledge over another's.
 * * **Typed failures.** Everything is normalized to `EngineError`, so the
 *   interface can say which state it is in — no vault, stale index, protocol
 *   mismatch, engine unreachable — and offer the action that fits.
 *
 * It sends no knowledge of its own: no graph is kept here, no vault file is
 * parsed, and the SQLite index is never opened.
 */
import { invoke } from "@tauri-apps/api/core"
import { normalizePath } from "@/lib/path-utils"
import {
  engineError,
  parseEngineAccepted,
  parseEngineNode,
  parseEngineResponse,
  parseLibraryLink,
  briefRequest,
  historyRequest,
  openRequest,
  indexRequest,
  pathRequest,
  proposalsRequest,
  reviewRequest,
  sourcesRequest,
  statusRequest,
  type EngineAccepted,
  type EngineContext,
  type EngineFinding,
  type EngineGraphPath,
  type EngineIndexReport,
  type EngineNode,
  type EngineNodeDetail,
  type EngineProposal,
  type EngineRequest,
  type EngineResponse,
  type EngineSource,
  type EngineStatus,
  type EngineVersion,
  type IndexScope,
  type LibraryLink,
} from "@/lib/engine-client"

/** The edit a person made to a proposal before accepting it. */
export interface ProposalEdit {
  title: string
  summary: string
  content: string
  reason?: string
}

const inFlight = new Map<string, Promise<EngineResponse>>()
const generations = new Map<string, number>()

function key(workspace: string): string {
  return normalizePath(workspace)
}

/** The generation a caller should capture before it starts waiting. */
export function engineGeneration(workspace: string): number {
  return generations.get(key(workspace)) ?? 0
}

/**
 * Forget every in-flight answer for a workspace.
 *
 * Called when a project closes or when the person asks for a reload: work
 * already on its way is no longer wanted, and its answer must not be applied.
 */
export function invalidateEngine(workspace?: string): void {
  if (workspace === undefined) {
    generations.clear()
    inFlight.clear()
    return
  }
  const normalized = key(workspace)
  generations.set(normalized, (generations.get(normalized) ?? 0) + 1)
  for (const entry of [...inFlight.keys()]) {
    if (entry.startsWith(`${normalized}\u0000`)) inFlight.delete(entry)
  }
}

/** True while an answer started at `generation` is still the one wanted. */
export function isEngineCurrent(workspace: string, generation: number): boolean {
  return engineGeneration(workspace) === generation
}

async function sendRequest(workspace: string, request: EngineRequest): Promise<EngineResponse> {
  const normalized = key(workspace)
  const entry = `${normalized}\u0000${engineGeneration(normalized)}\u0000${JSON.stringify(request)}`
  const pending = inFlight.get(entry)
  if (pending) return pending

  const run: Promise<EngineResponse> = invoke<unknown>("engine_request", {
    workspace: normalized,
    request,
  })
    .then(parseEngineResponse)
    .catch((cause) => {
      throw engineError(cause)
    })
    .finally(() => {
      inFlight.delete(entry)
    })
  inFlight.set(entry, run)
  return run
}

export async function engineStatus(workspace: string): Promise<EngineStatus> {
  const response = await sendRequest(workspace, statusRequest())
  if (response.method !== "status") {
    throw engineError(`engine_protocol_error: asked for status, received ${response.method}`)
  }
  return response.status
}

export async function engineReview(workspace: string, limit = 200): Promise<EngineFinding[]> {
  const response = await sendRequest(workspace, reviewRequest(limit))
  if (response.method !== "review") {
    throw engineError(`engine_protocol_error: asked for review, received ${response.method}`)
  }
  return response.findings
}

export async function engineSources(workspace: string, limit = 500): Promise<EngineSource[]> {
  const response = await sendRequest(workspace, sourcesRequest(limit))
  if (response.method !== "sources") {
    throw engineError(`engine_protocol_error: asked for sources, received ${response.method}`)
  }
  return response.sources
}

export async function engineIndex(
  workspace: string,
  options: { scope?: IndexScope; paths?: string[] } = {},
): Promise<EngineIndexReport> {
  const response = await sendRequest(workspace, indexRequest(options))
  if (response.method !== "index") {
    throw engineError(`engine_protocol_error: asked for index, received ${response.method}`)
  }
  return response.index
}

export async function engineProposals(workspace: string, limit = 100): Promise<EngineProposal[]> {
  const response = await sendRequest(workspace, proposalsRequest(limit))
  if (response.method !== "proposals") {
    throw engineError(`engine_protocol_error: asked for proposals, received ${response.method}`)
  }
  return response.proposals
}

/** A brief is composed by the engine; the desktop only asks and displays it. */
export async function engineBrief(
  workspace: string,
  task: string,
  options: { depth?: number; maxNodes?: number } = {},
): Promise<EngineContext> {
  const response = await sendRequest(workspace, briefRequest(task, options))
  if (response.method !== "context") {
    throw engineError(`engine_protocol_error: asked for a brief, received ${response.method}`)
  }
  return response.context
}

/** One node with its relationships, evidence, history, utility and page. */
export async function engineNodeDetail(
  workspace: string,
  node: string,
): Promise<EngineNodeDetail> {
  const response = await sendRequest(workspace, openRequest(node))
  if (response.method !== "open") {
    throw engineError(`engine_protocol_error: asked for a node, received ${response.method}`)
  }
  return response.detail
}

export async function engineNodeHistory(workspace: string, node: string): Promise<EngineVersion[]> {
  const response = await sendRequest(workspace, historyRequest(node))
  if (response.method !== "history") {
    throw engineError(`engine_protocol_error: asked for history, received ${response.method}`)
  }
  return response.versions
}

/** How two nodes connect, according to the engine's graph. */
export async function engineGraphPath(
  workspace: string,
  from: string,
  to: string,
  options: { depth?: number; maxNodes?: number } = {},
): Promise<EngineGraphPath> {
  const response = await sendRequest(workspace, pathRequest(from, to, options))
  if (response.method !== "path") {
    throw engineError(`engine_protocol_error: asked for a path, received ${response.method}`)
  }
  return response.path
}

/**
 * Accept a proposal. This is the only call in the app that can turn a model's
 * proposal into durable Markdown, and it does not write anything itself: the
 * engine writes the page. `edit` is what the person approved, when they
 * changed the text before deciding.
 */
export async function engineAccept(
  workspace: string,
  node: string,
  edit?: ProposalEdit,
): Promise<EngineAccepted> {
  try {
    const accepted = await invoke<unknown>("engine_accept", {
      workspace: key(workspace),
      node,
      edit: edit ?? null,
    })
    return parseEngineAccepted(accepted)
  } catch (cause) {
    throw engineError(cause)
  }
}

/** Reject a proposal. Writes nothing durable. */
export async function engineReject(
  workspace: string,
  node: string,
  reason?: string,
): Promise<EngineNode> {
  try {
    const rejected = await invoke<unknown>("engine_reject", {
      workspace: key(workspace),
      node,
      reason: reason ?? null,
    })
    return parseEngineNode(rejected)
  } catch (cause) {
    throw engineError(cause)
  }
}

/**
 * Which Library repository the project is linked to. Files only, no network:
 * this window never contacts the hosted Library, so what it can honestly say
 * is what is linked, not whether the Library is up.
 */
export async function libraryLink(workspace: string): Promise<LibraryLink> {
  try {
    const link = await invoke<unknown>("library_link", { workspace: key(workspace) })
    return parseLibraryLink(link)
  } catch (cause) {
    throw engineError(cause)
  }
}

/** Re-exported so panels do not import two modules to catch one error. */
export { engineError, EngineError } from "@/lib/engine-client"
