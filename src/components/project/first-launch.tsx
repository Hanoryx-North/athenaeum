import { useCallback, useEffect, useRef, useState } from "react"
import { LibraryAccount } from "@/components/setup/library-account"
import { Button } from "@/components/ui/button"
import { useLibraryAccountStore } from "@/stores/library-account-store"

export function FirstLaunch({ onComplete }: { onComplete: () => Promise<void> }) {
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const account = useLibraryAccountStore()
  const completing = useRef(false)
  const completed = useRef(false)

  const finish = useCallback(async () => {
    if (completing.current || completed.current) return
    completing.current = true
    setSaving(true)
    setError(null)
    try {
      await onComplete()
      completed.current = true
    } catch (cause) {
      setError(String(cause))
    } finally {
      completing.current = false
      setSaving(false)
    }
  }, [onComplete])

  useEffect(() => {
    if (account.checked && !account.checking && !account.error && account.username) void finish()
  }, [account.checked, account.checking, account.error, account.username, finish])

  return (
    <main className="flex h-full overflow-y-auto bg-background px-6 py-10 text-foreground">
      <div className="m-auto w-full max-w-sm space-y-8">
        <header className="space-y-3">
          <h1 className="text-2xl font-semibold">Sign in to Athenaeum</h1>
        </header>
        {saving ? <p role="status" className="text-sm text-muted-foreground">Opening wiki…</p> : <LibraryAccount />}
        {error && <div className="space-y-3">
          <p role="alert" className="text-sm text-destructive">Could not open your wiki. {error}</p>
          <Button onClick={() => void finish()}>Retry</Button>
        </div>}
      </div>
    </main>
  )
}
