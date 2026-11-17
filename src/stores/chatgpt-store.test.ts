import { afterEach, beforeEach, describe, expect, it, vi } from "vitest"

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }))
vi.mock("@/lib/chatgpt", () => ({ chatgptStatus: vi.fn(), chatgptSignIn: vi.fn(), chatgptCancelSignIn: vi.fn(), chatgptSignOut: vi.fn() }))
import { openUrl } from "@tauri-apps/plugin-opener"
import { chatgptStatus, chatgptSignIn, chatgptCancelSignIn } from "@/lib/chatgpt"
import { useChatgptStore } from "./chatgpt-store"
import { useAgentRuntimeStore } from "./agent-runtime-store"

const account = { type: "chatgpt" as const, email: "test@example.com" }
const login = { loginId: "login-1", authUrl: "https://auth.openai.com/authorize?state=test" }

describe("Codex connection lifecycle", () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.resetAllMocks()
    useChatgptStore.setState({ workspace: null, account: null, login: null, error: null, busy: false })
    vi.mocked(chatgptSignIn).mockResolvedValue(login)
    vi.mocked(chatgptStatus).mockResolvedValue({ account: null, login: { loginId: login.loginId } })
    vi.mocked(chatgptCancelSignIn).mockResolvedValue()
  })
  afterEach(() => { vi.clearAllTimers(); vi.useRealTimers() })

  it("opens the browser and selects Codex when the callback completes without another click", async () => {
    await useChatgptStore.getState().signIn("/project")
    expect(openUrl).toHaveBeenCalledWith(login.authUrl)
    expect(useChatgptStore.getState().busy).toBe(false)
    vi.mocked(chatgptStatus).mockResolvedValue({ account })
    await vi.advanceTimersByTimeAsync(1500)
    expect(useChatgptStore.getState()).toMatchObject({ account, login: null, error: null })
    expect(useAgentRuntimeStore.getState().runtime).toBe("chatgpt")
    await vi.advanceTimersByTimeAsync(60_000)
    expect(chatgptStatus).toHaveBeenCalledTimes(1)
  })

  it("keeps the sign-in URL available when the browser cannot open", async () => {
    vi.mocked(openUrl).mockRejectedValue(new Error("No opener"))
    await useChatgptStore.getState().signIn("/project")
    expect(useChatgptStore.getState().login?.url).toBe(login.authUrl)
    expect(useChatgptStore.getState().error).toContain("Could not open your browser")
  })

  it("ends an expired or lost attempt with recovery instead of polling forever", async () => {
    await useChatgptStore.getState().signIn("/project")
    vi.mocked(chatgptStatus).mockResolvedValue({ account: null, login: null })
    await vi.advanceTimersByTimeAsync(1500)
    expect(useChatgptStore.getState()).toMatchObject({ login: null, error: "Sign-in expired. Connect again." })
    expect(chatgptCancelSignIn).toHaveBeenCalledWith("/project", "login-1")
    await vi.advanceTimersByTimeAsync(60_000)
    expect(chatgptStatus).toHaveBeenCalledTimes(1)
  })

  it("stops polling after a transport failure and preserves the error", async () => {
    await useChatgptStore.getState().signIn("/project")
    vi.mocked(chatgptStatus).mockRejectedValue(new Error("The connection timed out. Try connecting again."))
    await vi.advanceTimersByTimeAsync(1500)
    expect(useChatgptStore.getState().login).toBeNull()
    expect(useChatgptStore.getState().error).toContain("timed out")
    await vi.advanceTimersByTimeAsync(60_000)
    expect(chatgptStatus).toHaveBeenCalledTimes(1)
  })

  it("ignores a status reply that arrives after cancellation", async () => {
    await useChatgptStore.getState().signIn("/project")
    let respond!: (value: { account: typeof account }) => void
    vi.mocked(chatgptStatus).mockImplementation(() => new Promise(resolve => { respond = resolve }))
    const checking = useChatgptStore.getState().check("/project")
    await useChatgptStore.getState().cancel()
    respond({ account })
    await checking
    expect(useChatgptStore.getState()).toMatchObject({ account: null, login: null, busy: false })
  })
})
