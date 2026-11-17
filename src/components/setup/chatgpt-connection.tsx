import { useEffect } from "react"
import { CheckCircle2, Loader2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useChatgptStore } from "@/stores/chatgpt-store"
import { useAgentRuntimeStore } from "@/stores/agent-runtime-store"
import { useWikiStore } from "@/stores/wiki-store"

export function ChatgptConnection({ workspace }: { workspace: string }) {
  const state = useChatgptStore()
  const account = state.workspace === workspace ? state.account : null
  const login = state.workspace === workspace ? state.login : null
  const error = state.workspace === workspace ? state.error : null
  useEffect(() => { void state.check(workspace) }, [workspace, state.check])

  function openChat() {
    useAgentRuntimeStore.getState().setRuntime("chatgpt")
    useWikiStore.getState().setActiveView("chat")
  }

  return <section className="space-y-4" aria-label="Codex connection">
    <h2 className="text-lg font-medium">Codex</h2>
    {account ? <>
      <p role="status" className="flex items-center gap-2 text-sm"><CheckCircle2 className="h-4 w-4" />{account.email || "Connected with ChatGPT"}</p>
      <div className="flex items-center gap-3">
        <Button onClick={openChat}>Open chat</Button>
        <Button variant="ghost" disabled={state.busy} onClick={() => void state.signOut(workspace)}>Sign out</Button>
      </div>
    </> : login ? <>
      <p role="status" className="text-sm text-muted-foreground">Finish signing in in your browser.</p>
      <div className="flex gap-3">
        <Button variant="outline" onClick={() => void state.openSignInPage()}>Open sign-in page</Button>
        <Button variant="ghost" disabled={state.busy} onClick={() => void state.cancel()}>Cancel</Button>
      </div>
      {error && <a href={login.url} target="_blank" rel="noreferrer" className="block break-all text-sm underline underline-offset-4">{login.url}</a>}
    </> : <Button disabled={state.busy} onClick={() => void state.signIn(workspace)}>
      {state.busy && <Loader2 className="h-4 w-4 animate-spin" />}
      {state.busy ? "Opening sign-in…" : "Connect with ChatGPT"}
    </Button>}
    {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
  </section>
}
