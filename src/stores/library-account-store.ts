import { create } from "zustand"
import { cliStatus } from "@/lib/cli-tools"
import { useSetupStore } from "@/stores/setup-store"

export const LIBRARY = "https://wiki.hanoryx.com"

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
      // The native CLI runner accepts one job at a time. Opening the account
      // control during project startup must wait for the agent doctor to finish.
      if (useSetupStore.getState().busy) {
        await new Promise<void>((resolve) => {
          const stop = useSetupStore.subscribe((state) => {
            if (!state.busy) { stop(); resolve() }
          })
        })
      }
      if (!(await cliStatus()).found) { set({ username: null }); return }
      const result = await useSetupStore.getState().operate(null, { job: "library_status", server: LIBRARY })
      if (result?.exitCode !== 0 || !Array.isArray(result.output)) throw new Error("Could not check your Library account. Try again.")
      const row = result.output.find((row) => row && typeof row.username === "string" && !row.detail)
      set({ username: row?.username ?? null })
    } catch (cause) {
      set({ error: cause instanceof Error ? cause.message : String(cause) })
    } finally {
      set({ checked: true, checking: false })
    }
  },
}))
