import { invoke } from "@tauri-apps/api/core"
import type { BridgeRequest, BridgeMethod, BridgeResponseMap } from "@athenaeum/protocol/bridge"

/** The desktop's one door to the separately owned local core. */
export async function coreBridge<M extends BridgeMethod>(
  workspace: string,
  request: Extract<BridgeRequest, { method: M }>,
): Promise<BridgeResponseMap[M]> {
  return invoke<BridgeResponseMap[M]>("core_bridge", { workspace, request })
}

export async function memoryBrief(workspace: string, task: string) {
  const response = await coreBridge(workspace, { method: "context", task, depth: 2, maxNodes: 8 })
  return response.context
}
