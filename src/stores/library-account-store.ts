import { create } from "zustand"
import { cliStatus } from "@/lib/cli-tools"
import { useSetupStore } from "@/stores/setup-store"

export const LIBRARY = "https://wiki.hanoryx.com"

export async function signInToLibrary(username: string, password: string, server = LIBRARY): Promise<string> {
  if (!username.trim() || !password) throw new Error("Enter your username and password.")
  if (useSetupStore.getState().busy) throw new Error("Another account or sync operation is running. Try again.")
  if (!(await cliStatus()).found && !(await useSetupStore.getState().install(null))) {
    throw new Error(useSetupStore.getState().error ?? "Could not prepare sign-in. Try again.")
  }
  const result = await useSetupStore.getState().operate(null, {
    job: "library_sign_in", server, username: username.trim(), password,
  })
  const output = result?.output as { username?: unknown } | null
  if (result?.exitCode !== 0 || typeof output?.username !== "string" || !output.username.trim()) {
    throw new Error(useSetupStore.getState().error ?? "Sign-in did not finish. Try again.")
  }
  if (server === LIBRARY) useLibraryAccountStore.getState().setUsername(output.username)
  return output.username
}

interface LibraryAccountState {
  username: string | null
  checked: boolean
  checking: boolean
  error: string | null
  refresh: () => Promise<void>
  setUsername: (username: string | null) => void
}

/** Identity comes from the CLI's saved credentials, never a cached UI success flag. */
export const useLibraryAccountStore = create<LibraryAccountState>((set, get) => ({
  username: null,
  checked: false,
  checking: false,
  error: null,
  setUsername: (username) => set({ username, checked: true, error: null }),
  refresh: async () => {
    if (get().checking) return
    set({ checking: true, error: null })
    try {
      if (!(await cliStatus()).found) { set({ username: null }); return }
      const result = await useSetupStore.getState().operate(null, { job: "library_status", server: LIBRARY })
      if (result?.exitCode !== 0 || !Array.isArray(result.output)) throw new Error(useSetupStore.getState().error || "Could not check your Library account. Try again.")
      const problem = result.output.find(row => row?.detail)
      if (problem) {
        if (String(problem.detail).startsWith("unauthorized:")) {
          set({ username: null, error: get().username ? "Session expired. Sign in again." : null })
          return
        }
        throw new Error(problem.reachable === false ? "Library unavailable. Try again." : String(problem.detail))
      }
      const row = result.output.find((row) => row && typeof row.username === "string")
      set({ username: row?.username ?? null })
    } catch (cause) {
      set({ error: cause instanceof Error ? cause.message : String(cause) })
    } finally {
      set({ checked: true, checking: false })
    }
  },
}))
