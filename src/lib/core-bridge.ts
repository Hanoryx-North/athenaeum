import { invoke } from "@tauri-apps/api/core"
import type { BridgeRequest, BridgeResponseMap, BridgeRecallSelection } from "@athenaeum/protocol/bridge"

export async function coreBridge<R extends BridgeRequest>(
  workspace: string,
  request: R,
): Promise<BridgeResponseMap[R["method"]]> {
  return invoke<BridgeResponseMap[R["method"]]>("core_bridge", { workspace, request })
}

export async function memoryBrief(workspace: string, task: string, selection?: BridgeRecallSelection) {
  const response = await coreBridge(workspace, { method: "context", task, depth: 2, maxNodes: 8, selection })
  return response.context
}

/** What the agent has proposed through the bridge that nobody has reviewed yet. */
export async function memoryProposals(workspace: string, limit = 20) {
  const response = await coreBridge(workspace, { method: "proposals", limit })
  return response.proposals
}

/** The desktop acts for the person at the keyboard, not for a model. */
const REVIEWER = { model: "athenaeum-desktop" }

/**
 * Accepting writes the proposal into the vault as a new Markdown page. It never overwrites a file,
 * and the page keeps the unverified authority it arrived with: accepting is not verifying.
 */
export async function acceptProposal(workspace: string, node: string) {
  const response = await coreBridge(workspace, { method: "accept", node, caller: REVIEWER })
  return response.accepted
}

export async function rejectProposal(workspace: string, node: string) {
  const response = await coreBridge(workspace, { method: "reject", node, caller: REVIEWER })
  return response.node
}
