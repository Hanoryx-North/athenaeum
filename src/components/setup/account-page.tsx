import { useEffect, useRef, useState } from "react"
import { ArrowLeft, ExternalLink } from "lucide-react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { AuthFrame } from "@/design-system/components"
import { LibraryAccount, LIBRARY } from "./library-account"
import { useLibraryAccountStore } from "@/stores/library-account-store"
import { useAccountViewStore } from "@/stores/account-view-store"

export function AccountPage() {
  const setOpen = useAccountViewStore((s) => s.setOpen)
  const username = useLibraryAccountStore((s) => s.username)
  const back = useRef<HTMLButtonElement>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    back.current?.focus()
  }, [])

  async function visit(path: string) {
    setError(null)
    try {
      await openUrl(`${LIBRARY}${path}`)
    } catch {
      setError("Could not open your browser. Try again.")
    }
  }

  return (
    <main className="desktop-account-page">
      <div className="desktop-account-toolbar">
        <Button ref={back} variant="ghost" onClick={() => setOpen(false)}>
          <ArrowLeft className="h-4 w-4" />
          Back to workspace
        </Button>
      </div>
      <AuthFrame>
        <div className="desktop-account-form">
          <h1>{username ? "Your account" : "Sign in to Library"}</h1>
          <LibraryAccount showHeading={false} />
          {username && (
            <nav className="desktop-profile-links" aria-label="Account">
              <button
                onClick={() =>
                  void visit(`/user/${encodeURIComponent(username)}`)
                }
              >
                Your profile
                <ExternalLink className="h-4 w-4" />
              </button>
              <button onClick={() => void visit("/settings")}>
                Account settings
                <ExternalLink className="h-4 w-4" />
              </button>
              <button onClick={() => void visit("/settings/security")}>
                Security
                <ExternalLink className="h-4 w-4" />
              </button>
            </nav>
          )}
          {error && (
            <p role="alert" className="text-destructive">
              {error}
            </p>
          )}
        </div>
      </AuthFrame>
    </main>
  )
}
