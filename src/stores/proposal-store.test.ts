import { beforeEach, describe, expect, it, vi } from "vitest"
vi.mock("@/lib/engine", () => ({ engineReject: vi.fn(), engineProposals: vi.fn() }))
import { engineProposals, engineReject } from "@/lib/engine"
import type { EngineProposal } from "@/lib/engine-client"
import { useProposalStore } from "./proposal-store"

const pending = [{ node: { id: "pending" } }] as EngineProposal[]
const deferred = <T>() => {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(done => { resolve = done })
  return { promise, resolve }
}

describe("pending wiki pages", () => {
  beforeEach(() => {
    vi.resetAllMocks()
    useProposalStore.getState().detach()
    vi.mocked(engineProposals).mockResolvedValue(pending)
  })

  it("preserves a failed discard and allows retry", async () => {
    useProposalStore.setState({ workspace: "/project", proposals: pending })
    vi.mocked(engineReject).mockRejectedValueOnce(new Error("Could not discard. Try again."))
    await useProposalStore.getState().reject("pending")
    expect(useProposalStore.getState().proposals).toEqual(pending)
    expect(useProposalStore.getState().failures.pending).toContain("Could not discard")
    vi.mocked(engineReject).mockResolvedValue(pending[0].node)
    await useProposalStore.getState().reject("pending")
    expect(useProposalStore.getState().proposals).toEqual([])
    expect(useProposalStore.getState().busy.pending).toBe(false)
  })

  it("does not resurrect a discarded page from an earlier read", async () => {
    const response = deferred<EngineProposal[]>()
    vi.mocked(engineProposals).mockReturnValue(response.promise)
    useProposalStore.setState({ workspace: "/project", proposals: pending })
    const reading = useProposalStore.getState().load()
    vi.mocked(engineReject).mockResolvedValue(pending[0].node)
    await useProposalStore.getState().reject("pending")
    response.resolve(pending)
    await reading
    expect(useProposalStore.getState().proposals).toEqual([])
    expect(useProposalStore.getState().loading).toBe(false)
  })

  it("ignores replies from a closed workspace even when that folder is reopened", async () => {
    const first = deferred<EngineProposal[]>(), second = deferred<EngineProposal[]>()
    vi.mocked(engineProposals).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
    useProposalStore.getState().attach("/project")
    useProposalStore.getState().detach()
    useProposalStore.getState().attach("/project")
    second.resolve([])
    await vi.waitFor(() => expect(useProposalStore.getState().loading).toBe(false))
    first.resolve(pending)
    await first.promise
    expect(useProposalStore.getState().proposals).toEqual([])
  })
})
