import { create } from "zustand"
import { engineReject, engineProposals } from "@/lib/engine"
import { engineError, type EngineProposal } from "@/lib/engine-client"

interface ProposalStoreState {
  workspace: string | null
  proposals: EngineProposal[]
  loading: boolean
  error: string | null
  busy: Record<string, boolean>
  failures: Record<string, string>
  attach: (workspace: string) => void
  detach: () => void
  load: (options?: { force?: boolean }) => Promise<void>
  reject: (node: string, reason?: string) => Promise<void>
}

const empty = () => ({ proposals: [], loading: false, error: null, busy: {}, failures: {} })
let generation = 0
let read = 0

/** Pending pages only. The engine owns saving; accepted pages belong in the editor. */
export const useProposalStore = create<ProposalStoreState>((set, get) => ({
  workspace: null, ...empty(),
  attach: (workspace) => {
    if (get().workspace === workspace) return
    ++generation
    set({ workspace, ...empty() })
    void get().load()
  },
  detach: () => { ++generation; set({ workspace: null, ...empty() }) },
  load: async (options = {}) => {
    const workspace = get().workspace
    if (!workspace || get().loading && !options.force) return
    const version = generation, request = ++read
    set({ loading: true, error: null })
    try {
      const proposals = await engineProposals(workspace)
      if (version === generation && request === read) set({ proposals, loading: false })
    } catch (cause) {
      if (version === generation && request === read) set({ loading: false, error: engineError(cause).message })
    }
  },
  reject: async (node, reason) => {
    const workspace = get().workspace, version = generation
    if (!workspace || get().busy[node]) return
    set(state => ({ busy: { ...state.busy, [node]: true }, failures: { ...state.failures, [node]: "" } }))
    try {
      await engineReject(workspace, node, reason)
      if (version !== generation) return
      ++read
      set(state => ({ proposals: state.proposals.filter(proposal => proposal.node.id !== node), loading: false }))
    } catch (cause) {
      if (version === generation) set(state => ({ failures: { ...state.failures, [node]: engineError(cause).message } }))
    } finally {
      if (version === generation) set(state => ({ busy: { ...state.busy, [node]: false } }))
    }
  },
}))
