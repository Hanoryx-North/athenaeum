import { create } from "zustand"
import { chatgptCancelSignIn, chatgptSignIn, chatgptSignOut, chatgptStatus, type ChatgptAccount } from "@/lib/chatgpt"

interface ChatgptState {
  workspace: string | null
  account: ChatgptAccount | null
  login: { id: string; url: string } | null
  busy: boolean
  error: string | null
  check: (workspace: string) => Promise<void>
  signIn: (workspace: string) => Promise<void>
  openSignInPage: () => Promise<void>
  cancel: () => Promise<void>
  signOut: (workspace: string) => Promise<void>
}
const message = (error: unknown) => error instanceof Error ? error.message : String(error)
let generation = 0
export const useChatgptStore = create<ChatgptState>((set, get) => ({
  workspace: null, account: null, login: null, busy: false, error: null,
  check: async (workspace) => {
    if (get().busy && get().workspace === workspace) return
    const version = ++generation
    if (get().workspace !== workspace) set({ workspace, account: null, login: null })
    set({ busy: true })
    try {
      const status = await chatgptStatus(workspace)
      if (version !== generation) return
      set({ account: status.account, error: status.login?.success === false ? status.login.error || "ChatGPT sign-in did not finish. Try again." : null,
        ...(status.account || status.login?.success === false ? { login: null } : {}) })
    } catch (error) { if (version === generation) set({ account: null, error: message(error) }) }
    finally { if (version === generation) set({ busy: false }) }
  },
  signIn: async (workspace) => {
    if (get().busy) return
    const version = ++generation
    set({ workspace, busy: true, error: null, account: null, login: null })
    try {
      const login = await chatgptSignIn(workspace)
      if (version !== generation) return
      set({ login: { id: login.loginId, url: login.authUrl } })
      await get().openSignInPage()
    } catch (error) { if (version === generation) set({ error: message(error) }) }
    finally { if (version === generation) set({ busy: false }) }
  },
  openSignInPage: async () => {
    const login = get().login
    if (!login) return
    try {
      const { openUrl } = await import("@tauri-apps/plugin-opener")
      await openUrl(login.url)
    } catch (error) { set({ error: message(error) }) }
  },
  cancel: async () => {
    const { workspace, login, busy } = get()
    if (!workspace || !login || busy) return
    ++generation
    set({ busy: true, error: null })
    try { await chatgptCancelSignIn(workspace, login.id); set({ login: null }) }
    catch (error) { set({ error: message(error) }) }
    finally { set({ busy: false }) }
  },
  signOut: async (workspace) => {
    if (get().busy) return
    ++generation
    set({ busy: true, error: null })
    try { await chatgptSignOut(workspace); set({ account: null, login: null }) }
    catch (error) { set({ error: message(error) }) }
    finally { set({ busy: false }) }
  },
}))
