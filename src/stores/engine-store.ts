/**
 * The knowledge health the engine reports, for one open project.
 *
 * One store, one workspace, one generation. Every read captures the
 * generation it started in and refuses to write if the project changed or the
 * person asked for a reload while it was in flight — the stale-response case —
 * and `src/lib/engine.ts` de-duplicates identical in-flight requests so two
 * panels opening at once make one round trip.
 *
 * The store holds what the engine said, and nothing it did not: no graph is
 * built here, no vault file is parsed, no index is read.
 */
import { create } from "zustand"
import {
  engineGeneration,
  engineIndex,
  engineReview,
  engineSources,
  engineStatus,
  invalidateEngine,
  isEngineCurrent,
  libraryLink,
} from "@/lib/engine"
import type {
  EngineFinding,
  EngineIndexReport,
  EngineSource,
  EngineStatus,
  LibraryLink,
} from "@/lib/engine-client"
import {
  engineStateFromError,
  engineStateFromStatus,
  libraryStateFrom,
  libraryStateFromError,
  type EngineProductState,
  type LibraryState,
} from "@/lib/engine-state"

interface EngineStoreState {
  workspace: string | null
  /** What the engine says about the workspace and its index. */
  status: EngineStatus | null
  state: EngineProductState
  statusError: string | null

  findings: EngineFinding[]
  findingsLoadedAt: number | null
  reviewError: string | null

  sources: EngineSource[]
  sourcesError: string | null

  indexReport: EngineIndexReport | null
  indexError: string | null

  library: LibraryLink | null
  libraryState: LibraryState
  libraryError: string | null

  loading: boolean
  refreshing: boolean
  loadedAt: number | null
  /** Set when a reload was refused because the project changed under it. */
  staleResponseDropped: boolean

  attach: (workspace: string) => void
  detach: () => void
  /**
   * Re-read the workspace. `findings` defaults to true; a vault write that
   * cannot have changed what needs deciding skips the review sweep, which is
   * the most expensive read the engine offers.
   */
  refresh: (options?: { findings?: boolean; deep?: boolean }) => Promise<void>
  /** Re-read just the review findings; a sweep over the whole knowledge base. */
  loadFindings: () => Promise<void>
  rebuildIndex: () => Promise<EngineIndexReport | null>
  loadLibrary: () => Promise<void>
  clearIndexReport: () => void
}

const EMPTY_STATE: Pick<
  EngineStoreState,
  | "status"
  | "state"
  | "statusError"
  | "findings"
  | "findingsLoadedAt"
  | "reviewError"
  | "sources"
  | "sourcesError"
  | "indexReport"
  | "indexError"
  | "library"
  | "libraryState"
  | "libraryError"
  | "loading"
  | "refreshing"
  | "loadedAt"
  | "staleResponseDropped"
> = {
  status: null,
  state: { kind: "loading" },
  statusError: null,
  findings: [],
  findingsLoadedAt: null,
  reviewError: null,
  sources: [],
  sourcesError: null,
  indexReport: null,
  indexError: null,
  library: null,
  libraryState: { kind: "loading" },
  libraryError: null,
  loading: false,
  refreshing: false,
  loadedAt: null,
  staleResponseDropped: false,
}

export const useEngineStore = create<EngineStoreState>((set, get) => ({
  workspace: null,
  ...EMPTY_STATE,

  /**
   * Point the store at a project. Switching projects drops everything the
   * previous one showed before the first new request is made, so no panel can
   * render one project's knowledge under another's name.
   */
  attach: (workspace) => {
    if (get().workspace === workspace) return
    if (get().workspace) invalidateEngine(get().workspace ?? undefined)
    set({ workspace, ...EMPTY_STATE, loading: true, state: { kind: "loading" } })
    void get().refresh()
  },

  detach: () => {
    const workspace = get().workspace
    if (workspace) invalidateEngine(workspace)
    set({ workspace: null, ...EMPTY_STATE })
  },

  /**
   * Reload from the engine. `deep` also asks for the index report, which is
   * the expensive one; a plain refresh reads status, review, sources and the
   * Library link.
   *
   * A reload supersedes anything in flight: the generation moves on, so the
   * older answer is dropped rather than applied late.
   */
  refresh: async (options = {}) => {
    const workspace = get().workspace
    if (!workspace) return
    const wantFindings = options.findings !== false
    invalidateEngine(workspace)
    const generation = engineGeneration(workspace)
    const current = () => isEngineCurrent(workspace, generation) && get().workspace === workspace

    set({
      loading: get().status === null,
      refreshing: true,
      statusError: null,
      reviewError: null,
      sourcesError: null,
      staleResponseDropped: false,
    })

    const status = await engineStatus(workspace).then(
      (value) => ({ ok: true as const, value }),
      (cause: unknown) => ({ ok: false as const, cause }),
    )
    if (!current()) {
      set({ staleResponseDropped: true })
      return
    }
    if (status.ok) {
      set({
        status: status.value,
        state: engineStateFromStatus(status.value),
        statusError: null,
      })
    } else {
      const state = engineStateFromError(status.cause)
      set({ status: null, state, statusError: state.message })
    }

    // Past an absent vault there is nothing to read, so the deeper reads are
    // skipped rather than reported as failures of their own.
    const blocking = !status.ok || status.value.initialized === false || status.value.hasVault === false

    const [review, sources, library] = await Promise.all([
      blocking || !wantFindings
        ? Promise.resolve({ ok: true as const, value: undefined })
        : engineReview(workspace).then(
            (value) => ({ ok: true as const, value }),
            (cause: unknown) => ({ ok: false as const, cause }),
          ),
      blocking
        ? Promise.resolve({ ok: true as const, value: [] as EngineSource[] })
        : engineSources(workspace).then(
            (value) => ({ ok: true as const, value }),
            (cause: unknown) => ({ ok: false as const, cause }),
          ),
      libraryLink(workspace).then(
        (value) => ({ ok: true as const, value }),
        (cause: unknown) => ({ ok: false as const, cause }),
      ),
    ])
    if (!current()) {
      set({ staleResponseDropped: true })
      return
    }

    // A refresh that skipped the sweep keeps what is on screen; a project with
    // no vault clears it, because those findings describe a vault that is gone.
    const findings = review.ok ? review.value : undefined
    set({
      findings: findings ?? (blocking ? [] : get().findings),
      findingsLoadedAt: findings !== undefined ? Date.now() : blocking ? null : get().findingsLoadedAt,
      reviewError: review.ok ? null : engineStateFromError(review.cause).message,
      sources: sources.ok ? sources.value : [],
      sourcesError: sources.ok ? null : engineStateFromError(sources.cause).message,
      library: library.ok ? library.value : null,
      libraryState: library.ok ? libraryStateFrom(library.value) : libraryStateFromError(library.cause),
      libraryError: library.ok ? null : libraryStateFromError(library.cause).message,
      loading: false,
      refreshing: false,
      loadedAt: Date.now(),
    })

    if (options.deep) await get().rebuildIndex()
  },

  /** Re-read the review findings on their own. */
  loadFindings: async () => {
    const workspace = get().workspace
    if (!workspace) return
    const generation = engineGeneration(workspace)
    set({ reviewError: null })
    try {
      const findings = await engineReview(workspace)
      if (!isEngineCurrent(workspace, generation) || get().workspace !== workspace) {
        set({ staleResponseDropped: true })
        return
      }
      set({ findings, findingsLoadedAt: Date.now(), reviewError: null })
    } catch (cause) {
      if (!isEngineCurrent(workspace, generation) || get().workspace !== workspace) return
      set({ reviewError: engineStateFromError(cause).message })
    }
  },

  /**
   * Ask the engine to reconcile the derived index. This is the recovery action
   * for a stale or missing index, and the only thing the UI does about it: the
   * engine decides what to rebuild, the UI only asks.
   */
  rebuildIndex: async () => {
    const workspace = get().workspace
    if (!workspace) return null
    const generation = engineGeneration(workspace)
    set({ indexError: null })
    try {
      const report = await engineIndex(workspace, { scope: "all" })
      if (!isEngineCurrent(workspace, generation) || get().workspace !== workspace) {
        set({ staleResponseDropped: true })
        return null
      }
      set({ indexReport: report, indexError: null })
      // The index moved, so what status and review would say has changed too.
      await get().refresh()
      return report
    } catch (cause) {
      if (!isEngineCurrent(workspace, generation) || get().workspace !== workspace) return null
      set({ indexError: engineStateFromError(cause).message })
      return null
    }
  },

  /**
   * Re-read the Library link on its own. Cheap enough to press twice: it reads
   * `athenaeum.json` and the sync state, and never touches the network.
   */
  loadLibrary: async () => {
    const workspace = get().workspace
    if (!workspace) return
    set({ libraryState: { kind: "loading" }, libraryError: null })
    try {
      const link = await libraryLink(workspace)
      if (get().workspace !== workspace) return
      set({ library: link, libraryState: libraryStateFrom(link), libraryError: null })
    } catch (cause) {
      if (get().workspace !== workspace) return
      const state = libraryStateFromError(cause)
      set({ library: null, libraryState: state, libraryError: state.message })
    }
  },

  clearIndexReport: () => set({ indexReport: null, indexError: null }),
}))
