import { useState } from "react"
import { Cloud, HardDrive, ArrowRight } from "lucide-react"
import { Button } from "@/components/ui/button"
import { LibraryAccount } from "@/components/setup/library-account"
import { cn } from "@/lib/utils"
import { useSetupStore } from "@/stores/setup-store"

export function FirstLaunch({ onComplete }: { onComplete: () => Promise<void> }) {
  const [choice, setChoice] = useState<"cloud" | "local">("cloud")
  const [page, setPage] = useState<"choice" | "account">("choice")
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const busy = useSetupStore((s) => s.busy)

  async function finish() {
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
      <div className="m-auto w-full max-w-lg space-y-8">
        <header className="space-y-3">
          <p className="text-sm text-muted-foreground">Welcome to Athenaeum</p>
          <h1 className="text-3xl font-semibold tracking-tight">Let’s get you connected.</h1>
          <p className="text-sm leading-relaxed text-muted-foreground">
            First, choose where to keep your wiki. Then open your project and connect your coding
            agent.
          </p>
        </header>
        {page === "choice" ? (
          <>
            <div className="space-y-3" role="radiogroup" aria-label="How to start">
              {(
                [
                  {
                    id: "cloud",
                    title: "Sign in to Athenaeum Library",
                    description: "Use your account to share accepted wiki pages between computers.",
                    icon: Cloud,
                  },
                  {
                    id: "local",
                    title: "Use this computer only",
                    description: "Start without an account. You can sign in later.",
                    icon: HardDrive,
                  },
                ] as const
              ).map(({ id, title, description, icon: Icon }) => (
                <button
                  key={id}
                  role="radio"
                  aria-checked={choice === id}
                  onClick={() => setChoice(id)}
                  className={cn(
                    "flex w-full items-start gap-4 rounded-xl border p-5 text-left focus-visible:ring-2 focus-visible:ring-ring",
                    choice === id ? "border-primary bg-primary/5" : "hover:bg-muted/40",
                  )}
                >
                  <Icon className="mt-0.5 h-5 w-5 shrink-0" />
                  <span>
                    <span className="block font-medium">{title}</span>
                    <span className="mt-1 block text-sm leading-relaxed text-muted-foreground">
                      {description}
                    </span>
                  </span>
                  <span
                    aria-hidden
                    className={cn(
                      "ml-auto mt-1 h-4 w-4 shrink-0 rounded-full border",
                      choice === id && "border-[5px] border-primary",
                    )}
                  />
                </button>
              ))}
            </div>
            <div className="flex justify-end">
              <Button
                disabled={saving}
                onClick={() => (choice === "cloud" ? setPage("account") : void finish())}
              >
                {saving ? "Opening…" : "Next"}
                <ArrowRight className="h-4 w-4" />
              </Button>
            </div>
          </>
        ) : (
          <>
            <LibraryAccount onContinue={() => void finish()} />
            <Button
              variant="ghost"
              disabled={saving || busy !== null}
              onClick={() => setPage("choice")}
            >
              Back
            </Button>
          </>
        )}
        {error && (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        )}
      </div>
    </main>
  )
}
