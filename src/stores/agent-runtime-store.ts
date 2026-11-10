import { create } from "zustand"
import { readUiPreference, saveUiPreference } from "@/lib/ui-preferences"

export type AgentRuntime = "native" | "chatgpt" | "harness"
const saved = readUiPreference<AgentRuntime>("chat-agent-runtime", "native")
export const useAgentRuntimeStore = create<{ runtime: AgentRuntime; setRuntime: (runtime: AgentRuntime) => void }>((set) => ({
  runtime: ["native", "chatgpt", "harness"].includes(saved) ? saved : "native",
  setRuntime: (runtime) => { saveUiPreference("chat-agent-runtime", runtime); set({ runtime }) },
}))
