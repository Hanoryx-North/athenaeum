import { useEffect, useState } from "react"
import { CheckCircle2, Loader2 } from "lucide-react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { cliStatus, runCli } from "@/lib/cli-tools"
import { useSetupStore } from "@/stores/setup-store"
import { LIBRARY, useLibraryAccountStore } from "@/stores/library-account-store"
import { useEngineStore } from "@/stores/engine-store"
import { useWikiStore } from "@/stores/wiki-store"

export { LIBRARY } from "@/stores/library-account-store"

/** Account identity. The workspace scheduler starts private backup after sign-in. */
export function LibraryAccount({
  onContinue,
  continuing = false,
  server = LIBRARY,
}: {
  onContinue?: () => void
  continuing?: boolean
  server?: string
}) {
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const account = useLibraryAccountStore()
  const projectPath = useWikiStore((state) => state.project?.path)
  const library = useEngineStore((state) => state.workspace === projectPath ? state.library : null)
  const [localSignedIn, setLocalSignedIn] = useState<string | null>(null)
  const signedIn = server === LIBRARY ? account.username : localSignedIn
  function setSignedIn(username: string | null) {
    if (server === LIBRARY) useLibraryAccountStore.getState().setUsername(username)
    else setLocalSignedIn(username)
  }
  const [checking, setChecking] = useState(true)
  const [message, setMessage] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)
  const busy = useSetupStore((s) => s.busy)
  const working = submitting || continuing || busy !== null

  useEffect(() => {
    let cancelled = false
    setChecking(true)
    setLocalSignedIn(null)
    async function check() {
      try {
        if (server === LIBRARY) {
          await useLibraryAccountStore.getState().refresh()
          return
        }
        if (!(await cliStatus()).found) return
        const result = await runCli(null, { job: "library_status", server })
        const rows = result.output
        if (!cancelled && result.exitCode === 0 && Array.isArray(rows)) {
          const row = rows.find((row) => row && typeof row.username === "string" && !row.detail)
          if (row) setSignedIn(row.username)
        }
      } catch {
        if (!cancelled) setMessage("Could not check your account. You can try signing in again.")
      } finally {
        if (!cancelled) setChecking(false)
      }
    }
    void check()
    return () => {
      cancelled = true
    }
  }, [server])

  async function signIn(event: React.FormEvent) {
    event.preventDefault()
    if (working) return
    setSubmitting(true)
    setMessage(null)
    // Clear the input immediately. It is never saved in desktop settings.
    const enteredPassword = password
    setPassword("")
    try {
      if (!(await cliStatus()).found && !(await useSetupStore.getState().install(null))) {
        setMessage(useSetupStore.getState().error ?? "Could not prepare sign-in. Try again.")
        return
      }
      const result = await useSetupStore
        .getState()
        .operate(null, {
          job: "library_sign_in",
          server,
          username: username.trim(),
          password: enteredPassword,
        })
      const output = result?.output as { username?: unknown } | null
      if (result?.exitCode === 0 && typeof output?.username === "string") {
        setSignedIn(output.username)
      } else {
        setMessage(useSetupStore.getState().error ?? "Sign-in did not finish. Try again.")
      }
    } catch (cause) {
      setMessage(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <section className="space-y-6" aria-label="Athenaeum Library account">
      <div>
        <h2 className="text-xl font-semibold">Your Library account</h2>
        <p className="mt-2 text-sm leading-relaxed text-muted-foreground">
          Sign in to back up your wiki to your server and use it across computers. New backups are private.
        </p>
      </div>
      {checking || (server === LIBRARY && account.checking) ? (
        <p role="status" className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Checking your account…
        </p>
      ) : signedIn ? (
        <>
          <div role="status" className="flex items-center gap-3 rounded-xl border bg-muted/20 p-5">
            <CheckCircle2 className="h-5 w-5 text-emerald-600" />
            <div>
              <p className="font-medium">Signed in as {signedIn}</p>
              <p className="mt-1 text-sm text-muted-foreground">Your account is connected.</p>
              {library?.server === server && library.lastSyncAt && (
                <p className="mt-1 text-xs text-muted-foreground">
                  This wiki last synced {new Date(library.lastSyncAt).toLocaleString()}.
                </p>
              )}
            </div>
          </div>
          <div className="flex items-center justify-between gap-3">
            <Button
              variant="ghost"
              disabled={working}
              onClick={() =>
                void (async () => {
                  const result = await useSetupStore
                    .getState()
                    .operate(null, { job: "library_logout", server })
                  if (result?.exitCode === 0) setSignedIn(null)
                  else setMessage(useSetupStore.getState().error)
                })()
              }
            >
              Sign out
            </Button>
            {onContinue && (
              <Button onClick={onContinue} disabled={working}>
                {continuing ? "Opening…" : "Open Athenaeum"}
              </Button>
            )}
          </div>
        </>
      ) : (
        <form onSubmit={(event) => void signIn(event)} className="space-y-4">
          <label className="block space-y-2 text-sm font-medium">
            <span>Username or email</span>
            <Input
              autoComplete="username"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              disabled={working}
              required
              maxLength={254}
            />
          </label>
          <label className="block space-y-2 text-sm font-medium">
            <span>Password</span>
            <Input
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              disabled={working}
              required
              maxLength={256}
            />
          </label>
          <Button
            type="submit"
            className="w-full"
            disabled={working || !username.trim() || !password}
          >
            {working && <Loader2 className="h-4 w-4 animate-spin" />}
            {working ? "Signing in…" : "Sign in"}
          </Button>
          <p className="text-center text-sm text-muted-foreground">
            New here?{" "}
            <button
              type="button"
              className="text-foreground underline underline-offset-4"
              disabled={working}
              onClick={() =>
                void openUrl(`${server}/register`).catch(() =>
                  setMessage(
                    "Could not open the browser. Visit the Library website to create your account.",
                  ),
                )
              }
            >
              Create an account
            </button>
          </p>
        </form>
      )}
      {(message || (server === LIBRARY && account.error)) && (
        <p role="alert" className="text-sm text-destructive">
          {message || account.error}
        </p>
      )}
    </section>
  )
}
