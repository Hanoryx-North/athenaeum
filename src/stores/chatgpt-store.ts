import { create } from "zustand"
import { openUrl } from "@tauri-apps/plugin-opener"
import { chatgptCancelSignIn, chatgptSignIn, chatgptSignOut, chatgptStatus, type ChatgptAccount } from "@/lib/chatgpt"
import { useAgentRuntimeStore } from "@/stores/agent-runtime-store"

interface ChatgptLogin { id: string; url: string; expiresAt: number }
interface ChatgptState {
  workspace: string | null
  account: ChatgptAccount | null
  login: ChatgptLogin | null
  busy: boolean
  error: string | null
  check: (workspace: string) => Promise<void>
  signIn: (workspace: string) => Promise<void>
  openSignInPage: () => Promise<void>
  cancel: () => Promise<void>
  signOut: (workspace: string) => Promise<void>
}

const message = (error: unknown) => error instanceof Error ? error.message : String(error)
const LOGIN_TIMEOUT_MS = 5 * 60_000
let generation = 0
let polling: ReturnType<typeof setTimeout> | undefined
let checkingWorkspace: string | null = null
function stopPolling() { clearTimeout(polling); polling = undefined }
function poll(workspace: string) {
  stopPolling()
  polling = setTimeout(() => void useChatgptStore.getState().check(workspace), 1500)
}

export const useChatgptStore = create<ChatgptState>((set, get) => ({
  workspace: null, account: null, login: null, busy: false, error: null,
  check: async (workspace) => {
    if (checkingWorkspace === workspace || get().busy) return
    if (get().workspace !== workspace) {
      stopPolling()
      ++generation
      set({ workspace, account: null, login: null, error: null })
    }
    const version = generation
    checkingWorkspace = workspace
    try {
      const status = await chatgptStatus(workspace)
      if (version !== generation) return
      const login = get().login
      if (status.account) {
        stopPolling()
        set({ account: status.account, login: null, error: null })
        if (login) useAgentRuntimeStore.getState().setRuntime("chatgpt")
      } else if (login) {
        if (status.login?.success === false || !status.login || status.login.loginId !== login.id || Date.now() >= login.expiresAt) {
          stopPolling()
          set({ account: null, login: null, error: status.login?.error || "Sign-in expired. Connect again." })
          await chatgptCancelSignIn(workspace, login.id).catch(() => {})
        } else { set({ account: null }); poll(workspace) }
      } else { set({ account: null, error: null }) }
    } catch (error) {
      if (version === generation) { stopPolling(); set({ login: null, error: message(error) }) }
    } finally { if (checkingWorkspace === workspace) checkingWorkspace = null }
  },
  signIn: async (workspace) => {
    if (get().busy) return
    stopPolling()
    const version = ++generation
    set({ workspace, busy: true, error: null, login: null })
    try {
      const result = await chatgptSignIn(workspace)
      if (version !== generation) return
      const url = new URL(result.authUrl)
      if (url.protocol !== "https:" || !result.loginId) throw new Error("Invalid sign-in response. Connect again.")
      set({ account: null, login: { id: result.loginId, url: result.authUrl, expiresAt: Date.now() + LOGIN_TIMEOUT_MS } })
      await get().openSignInPage()
    } catch (error) { if (version === generation) set({ error: message(error) }) }
    finally {
      if (version === generation) {
        set({ busy: false })
        if (get().login) poll(workspace)
      }
    }
  },
  openSignInPage: async () => {
    const login = get().login
    if (!login) return
    try { await openUrl(login.url); set({ error: null }) }
    catch { set({ error: "Could not open your browser. Open the sign-in page below." }) }
  },
  cancel: async () => {
    const { workspace, login, busy } = get()
    if (!workspace || !login || busy) return
    stopPolling()
    ++generation
    set({ busy: true, error: null, login: null })
    try { await chatgptCancelSignIn(workspace, login.id) }
    catch (error) { set({ error: message(error) }) }
    finally { set({ busy: false }) }
  },
  signOut: async (workspace) => {
    if (get().busy) return
    stopPolling()
    ++generation
    set({ busy: true, error: null })
    try { await chatgptSignOut(workspace); set({ account: null, login: null }) }
    catch (error) { set({ error: message(error) }) }
    finally { set({ busy: false }) }
  },
}))
