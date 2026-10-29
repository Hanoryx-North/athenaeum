import { useState } from "react"
import { Button } from "@/components/ui/button"
import { LibraryAccount } from "@/components/setup/library-account"
import { useSetupStore } from "@/stores/setup-store"

export function FirstLaunch({ onComplete }: { onComplete: () => Promise<void> }) {
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const busy = useSetupStore((s) => s.busy)

  async function finish() {
    if (saving) return
    setSaving(true)
    setError(null)
    try {
      await onComplete()
    } catch (cause) {
      setError(String(cause))
    } finally {
      setSaving(false)
    }
  }

  return (
    <main className="flex h-full overflow-y-auto bg-background px-6 py-10 text-foreground">
      <div className="m-auto w-full max-w-sm space-y-8">
        <header className="space-y-3">
          <h1 className="text-3xl font-semibold tracking-tight">Welcome to Athenaeum</h1>
          <p className="text-sm leading-relaxed text-muted-foreground">
            A wiki that grows with your work. Sign in, open a wiki, then connect the coding
            agent you already use.
          </p>
        </header>
        <LibraryAccount onContinue={() => void finish()} continuing={saving} />
        <div className="space-y-1 text-center">
          <Button variant="link" disabled={saving || busy !== null} onClick={() => void finish()}>
            {saving ? "Opening…" : "Start locally"}
          </Button>
          <p className="text-xs text-muted-foreground">No account needed. You can sign in later.</p>
        </div>
        {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      </div>
    </main>
  )
}
