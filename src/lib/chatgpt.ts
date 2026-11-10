import { invoke } from "@tauri-apps/api/core"

export interface ChatgptAccount { type: "chatgpt"; email?: string; planType?: string }
export interface ChatgptStatus {
  account: ChatgptAccount | null
  login?: { loginId: string; success?: boolean; error?: string | null } | null
}
export function chatgptStatus(projectPath: string): Promise<ChatgptStatus> {
  return invoke("chatgpt_account", { projectPath, action: "status" })
}
export function chatgptSignIn(projectPath: string): Promise<{ loginId: string; authUrl: string }> {
  return invoke("chatgpt_account", { projectPath, action: "start" })
}
export function chatgptCancelSignIn(projectPath: string, loginId: string): Promise<void> {
  return invoke("chatgpt_account", { projectPath, action: "cancel", loginId })
}
export function chatgptSignOut(projectPath: string): Promise<void> {
  return invoke("chatgpt_account", { projectPath, action: "logout" })
}
