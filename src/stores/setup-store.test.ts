import { beforeEach, describe, expect, it, vi } from "vitest"
vi.mock("@/lib/cli-tools", () => ({
  cancelCli: vi.fn().mockResolvedValue(true), cliStatus: vi.fn(), installTools: vi.fn(),
  onCliProgress: vi.fn().mockResolvedValue(() => {}), parseDoctor: vi.fn(), parseSetup: vi.fn(), runCli: vi.fn(),
}))
import { runCli } from "@/lib/cli-tools"
import { useSetupStore } from "./setup-store"

describe("CLI operation coordination", () => {
  beforeEach(() => {
    vi.mocked(runCli).mockReset()
    vi.mocked(runCli).mockResolvedValue({ exitCode: 0, output: [], stderr: [] })
    useSetupStore.setState({ busy: null, error: null })
  })

  it("waits for a background check instead of dropping an account action", async () => {
    useSetupStore.setState({ busy: "check" })
    const request = useSetupStore.getState().operate(null, { job: "library_status", server: "https://library.test" })
    expect(runCli).not.toHaveBeenCalled()
    useSetupStore.setState({ busy: null })
    expect(await request).toMatchObject({ exitCode: 0 })
    expect(runCli).toHaveBeenCalledTimes(1)
    expect(useSetupStore.getState().error).toBeNull()
  })

  it("serializes concurrent explicit operations", async () => {
    let complete!: (result: { exitCode: number; output: unknown[]; stderr: string[] }) => void
    vi.mocked(runCli).mockImplementationOnce(() => new Promise(resolve => { complete = resolve }))
    const first = useSetupStore.getState().operate(null, { job: "library_status", server: "https://one.test" })
    await vi.waitFor(() => expect(runCli).toHaveBeenCalledTimes(1))
    const second = useSetupStore.getState().operate(null, { job: "library_status", server: "https://two.test" })
    expect(runCli).toHaveBeenCalledTimes(1)
    complete({ exitCode: 0, output: [], stderr: [] })
    await Promise.all([first, second])
    expect(runCli).toHaveBeenCalledTimes(2)
  })

  it("does not start queued work after cancellation", async () => {
    useSetupStore.setState({ busy: "check" })
    const queued = useSetupStore.getState().operate("/parent", { job: "sync_clone", server: "https://library.test", remote: "owner/wiki", destination: "/parent/wiki" })
    await useSetupStore.getState().cancel()
    useSetupStore.setState({ busy: null })
    expect(await queued).toBeNull()
    expect(runCli).not.toHaveBeenCalled()
  })

  it("drops queued operations when the project is reset", async () => {
    useSetupStore.setState({ busy: "check" })
    const queued = useSetupStore.getState().operate("/old-project", { job: "sync" })
    await useSetupStore.getState().reset()
    expect(await queued).toBeNull()
    expect(runCli).not.toHaveBeenCalled()
  })
})
