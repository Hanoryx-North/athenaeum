import { useEffect } from "react"
import { CheckCircle2, Loader2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useChatgptStore } from "@/stores/chatgpt-store"

export function ChatgptConnection({ workspace }: { workspace: string }) {
  const { account, login, busy, error, check, signIn, openSignInPage, cancel, signOut } = useChatgptStore()
  useEffect(() => { void check(workspace) }, [workspace, check])
  useEffect(() => {
    if (!login) return
    const timer = setInterval(() => { void check(workspace) }, 2000)
    return () => clearInterval(timer)
  }, [workspace, login, check])
  return (
    <div className="space-y-3 rounded-xl border p-5 text-sm" aria-label="ChatGPT account">
      {account ? <>
        <p className="flex items-center gap-2"><CheckCircle2 className="h-4 w-4 text-emerald-600" />Signed in{account.email ? ` as ${account.email}` : " with ChatGPT"}{account.planType ? ` (${account.planType})` : ""}.</p>
        <p className="text-muted-foreground">Use your ChatGPT plan for tasks inside Athenaeum. Your plan's usage limits apply.</p>
        <Button variant="ghost" size="sm" disabled={busy} onClick={() => void signOut(workspace)}>Sign out of ChatGPT</Button>
        <p className="text-xs text-muted-foreground">Sign-in is shared with your local Codex clients. Signing out also signs them out.</p>
      </> : login ? <>
        <p role="status">Finish signing in with ChatGPT in your browser. This page updates when you're done.</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" onClick={() => void openSignInPage()}>Open sign-in page</Button>
          <Button variant="ghost" disabled={busy} onClick={() => void cancel()}>Cancel sign-in</Button>
        </div>
      </> : <>
        <p>Use your ChatGPT account for chat inside Athenaeum. You don't need an OpenAI API key.</p>
        <Button disabled={busy} onClick={() => void signIn(workspace)}>{busy && <Loader2 className="h-4 w-4 animate-spin" />}Sign in with ChatGPT</Button>
      </>}
      {error && <p role="alert" className="text-destructive">{error}</p>}
      {!login && <Button variant="ghost" size="sm" disabled={busy} onClick={() => void check(workspace)}>{busy ? "Checking account..." : "Check account"}</Button>}
    </div>
  )
}
