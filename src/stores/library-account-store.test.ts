import { afterEach, beforeEach, expect, it, vi } from "vitest"
const cli = vi.hoisted(() => ({ operate: vi.fn(), error: null }))
vi.mock("@/lib/cli-tools", () => ({ cliStatus: vi.fn().mockResolvedValue({ found: true }) }))
vi.mock("./setup-store", () => ({ useSetupStore: { getState: () => cli } }))
import { useLibraryAccountStore } from "./library-account-store"

beforeEach(() => { cli.operate.mockReset(); useLibraryAccountStore.setState({ username: "tester", checked: true, checking: false, error: null }) })
afterEach(() => vi.restoreAllMocks())
it("preserves a saved identity while the Library is unreachable", async () => {
  cli.operate.mockResolvedValue({ exitCode: 0, output: [{ username: null, reachable: false, detail: "offline: unavailable" }], stderr: [] })
  await useLibraryAccountStore.getState().refresh()
  expect(useLibraryAccountStore.getState()).toMatchObject({ username: "tester", checking: false, error: "Library unavailable. Try again." })
})
it("clears an identity only when the Library rejects its authentication", async () => {
  cli.operate.mockResolvedValue({ exitCode: 0, output: [{ username: null, reachable: true, detail: "unauthorized: token expired" }], stderr: [] })
  await useLibraryAccountStore.getState().refresh()
  expect(useLibraryAccountStore.getState()).toMatchObject({ username: null, error: "Session expired. Sign in again." })
})
it("shows a normal sign-in form for an account that is already signed out", async () => {
  useLibraryAccountStore.setState({ username: null })
  cli.operate.mockResolvedValue({ exitCode: 0, output: [{ username: null, reachable: true, detail: "unauthorized: sign in" }], stderr: [] })
  await useLibraryAccountStore.getState().refresh()
  expect(useLibraryAccountStore.getState()).toMatchObject({ username: null, error: null })
})
