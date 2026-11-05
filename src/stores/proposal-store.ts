/**
 * The review queue: what the engine says is proposed, and the decision a
 * person makes about it.
 *
 * Individual review lives here. Batch acceptance and the owner's automatic
 * saving policy live in automation-store; every path writes through the same
 * core page writer.
 *
 * Rejecting is the other half of the same rule: it writes nothing durable and
 * simply stops the engine offering the node again.
 */
import { create } from "zustand"
import { engineAccept, engineReject, engineProposals, type ProposalEdit } from "@/lib/engine"
import { engineError, type EngineAccepted, type EngineNode, type EnginePage, type EngineProposal } from "@/lib/engine-client"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"

/** What the person changed before deciding, if anything. */
export interface ProposalDraft extends ProposalEdit {}

export interface ProposalDecision {
  action: "accepted" | "rejected"
  at: number
  /** The page acceptance wrote, for an acceptance. */
  page?: EnginePage
  alreadyAccepted?: boolean
  relationshipsWritten?: number
  relationshipsLeftLocal?: string[]
  /** The node as the engine returned it. */
  node?: EngineNode
  /** Set when the decision failed; the proposal stays in the queue. */
  error?: string
}

function draftFrom(proposal: EngineProposal): ProposalDraft {
  return {
    title: proposal.node.title,
    summary: proposal.node.summary,
    content: proposal.node.content,
  }
}

function draftChanged(proposal: EngineProposal, draft: ProposalDraft): boolean {
  return (
    draft.title !== proposal.node.title ||
    draft.summary !== proposal.node.summary ||
    draft.content !== proposal.node.content
  )
}

interface ProposalStoreState {
  workspace: string | null
  proposals: EngineProposal[]
  loading: boolean
  loadedAt: number | null
  error: string | null
  /** Which proposal is being edited, if any. */
  editing: string | null
  drafts: Record<string, ProposalDraft>
  /** Which proposals have a decision in flight. */
  busy: Record<string, "accept" | "reject" | undefined>
  decisions: Record<string, ProposalDecision>

  attach: (workspace: string) => void
  detach: () => void
  load: (options?: { force?: boolean }) => Promise<void>
  beginEdit: (node: string) => void
  cancelEdit: (node: string) => void
  setDraft: (node: string, patch: Partial<ProposalDraft>) => void
  resetDraft: (node: string) => void
  accept: (node: string) => Promise<EngineAccepted | null>
  reject: (node: string, reason?: string) => Promise<void>
  clearDecision: (node: string) => void
}

const EMPTY: Pick<
  ProposalStoreState,
  "proposals" | "loading" | "loadedAt" | "error" | "editing" | "drafts" | "busy" | "decisions"
> = {
  proposals: [],
  loading: false,
  loadedAt: null,
  error: null,
  editing: null,
  drafts: {},
  busy: {},
  decisions: {},
}

export const useProposalStore = create<ProposalStoreState>((set, get) => ({
  workspace: null,
  ...EMPTY,

  attach: (workspace) => {
    if (get().workspace === workspace) return
    set({ workspace, ...EMPTY, loading: true })
    void get().load()
  },

  detach: () => set({ workspace: null, ...EMPTY }),

  /** Ask the engine what is proposed. Answers are for the current project only. */
  load: async (options = {}) => {
    const workspace = get().workspace
    if (!workspace) return
    if (!options.force && get().loading && get().proposals.length > 0) return
    set({ loading: true, error: null })
    try {
      const proposals = await engineProposals(workspace)
      if (get().workspace !== workspace) return
      // Keep any draft the person is still typing, and any decision they have
      // already seen, so a background refresh cannot throw either away.
      const drafts = { ...get().drafts }
      for (const proposal of proposals) {
        if (!(proposal.node.id in drafts)) drafts[proposal.node.id] = draftFrom(proposal)
      }
      set({ proposals, drafts, loading: false, loadedAt: Date.now(), error: null })
    } catch (cause) {
      if (get().workspace !== workspace) return
      set({ loading: false, error: engineError(cause).message })
    }
  },

  beginEdit: (node) => {
    const proposal = get().proposals.find((candidate) => candidate.node.id === node)
    set((state) => ({
      editing: node,
      drafts: proposal && !(node in state.drafts) ? { ...state.drafts, [node]: draftFrom(proposal) } : state.drafts,
    }))
  },

  cancelEdit: (node) => {
    const proposal = get().proposals.find((candidate) => candidate.node.id === node)
    set((state) => {
      const drafts = { ...state.drafts }
      if (proposal) drafts[node] = draftFrom(proposal)
      else delete drafts[node]
      return { editing: state.editing === node ? null : state.editing, drafts }
    })
  },

  setDraft: (node, patch) =>
    set((state) => ({
      drafts: {
        ...state.drafts,
        [node]: { ...(state.drafts[node] ?? { title: "", summary: "", content: "" }), ...patch },
      },
    })),

  resetDraft: (node) =>
    set((state) => {
      const proposal = get().proposals.find((candidate) => candidate.node.id === node)
      if (!proposal) return state
      return { drafts: { ...state.drafts, [node]: draftFrom(proposal) } }
    }),

  /**
   * Accept one proposal. Writes Markdown through the engine, which is the only
   * writer; an edit is sent with the decision, so what becomes durable is what
   * the person read and approved.
   */
  accept: async (node) => {
    const workspace = get().workspace
    if (!workspace) return null
    const proposal = get().proposals.find((candidate) => candidate.node.id === node)
    if (!proposal) return null
    if (get().busy[node]) return null

    const draft = get().drafts[node] ?? draftFrom(proposal)
    const edit = draftChanged(proposal, draft)
      ? {
          title: draft.title,
          summary: draft.summary,
          content: draft.content,
        }
      : undefined
    if (edit && !edit.title.trim()) {
      set((state) => ({
        decisions: {
          ...state.decisions,
          [node]: { action: "accepted", at: Date.now(), error: "A page needs a title." },
        },
      }))
      return null
    }

    set((state) => ({ busy: { ...state.busy, [node]: "accept" } }))
    try {
      const accepted = await engineAccept(workspace, node, edit)
      if (get().workspace !== workspace) return null
      set((state) => {
        const drafts = { ...state.drafts }
        delete drafts[node]
        return {
          proposals: state.proposals.filter((candidate) => candidate.node.id !== node),
          drafts,
          editing: state.editing === node ? null : state.editing,
          busy: { ...state.busy, [node]: undefined },
          decisions: {
            ...state.decisions,
            [node]: {
              action: "accepted",
              at: Date.now(),
              page: accepted.page,
              alreadyAccepted: accepted.alreadyAccepted,
              relationshipsWritten: accepted.relationshipsWritten,
              relationshipsLeftLocal: accepted.relationshipsLeftLocal,
              node: accepted.node,
            },
          },
        }
      })
      // The engine wrote a page; let the tree and everything watching the
      // vault see it. The store does not write the file itself.
      await refreshProjectFileTree(workspace, { bumpDataVersion: true }).catch(() => undefined)
      return accepted
    } catch (cause) {
      if (get().workspace !== workspace) return null
      set((state) => ({
        busy: { ...state.busy, [node]: undefined },
        decisions: {
          ...state.decisions,
          [node]: { action: "accepted", at: Date.now(), error: engineError(cause).message },
        },
      }))
      return null
    }
  },

  /** Reject one proposal: nothing durable is written, the node stops being offered. */
  reject: async (node, reason) => {
    const workspace = get().workspace
    if (!workspace) return
    if (get().busy[node]) return
    set((state) => ({ busy: { ...state.busy, [node]: "reject" } }))
    try {
      const rejected = await engineReject(workspace, node, reason)
      if (get().workspace !== workspace) return
      set((state) => {
        const drafts = { ...state.drafts }
        delete drafts[node]
        return {
          proposals: state.proposals.filter((candidate) => candidate.node.id !== node),
          drafts,
          editing: state.editing === node ? null : state.editing,
          busy: { ...state.busy, [node]: undefined },
          decisions: {
            ...state.decisions,
            [node]: { action: "rejected", at: Date.now(), node: rejected },
          },
        }
      })
    } catch (cause) {
      if (get().workspace !== workspace) return
      set((state) => ({
        busy: { ...state.busy, [node]: undefined },
        decisions: {
          ...state.decisions,
          [node]: { action: "rejected", at: Date.now(), error: engineError(cause).message },
        },
      }))
    }
  },

  clearDecision: (node) =>
    set((state) => {
      const decisions = { ...state.decisions }
      delete decisions[node]
      return { decisions }
    }),
}))
