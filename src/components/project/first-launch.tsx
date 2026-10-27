import { useRef, useState } from "react"
import { Cloud, HardDrive, Loader2 } from "lucide-react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { Button } from "@/components/ui/button"
import { cancelCli, cliStatus, installTools, onCliProgress, runCli, type JobResult } from "@/lib/cli-tools"

const LIBRARY = "https://wiki.hanoryx.com"

function requireSuccess(result: JobResult) {
  if (result.exitCode === 0) return
  const output = result.output as { error?: { message?: string }; message?: string } | null
  throw new Error(output?.error?.message ?? output?.message ?? result.stderr[result.stderr.length - 1] ?? "Sign-in stopped. Please try again.")
}

export function FirstLaunch({ onComplete }: { onComplete: () => Promise<void> }) {
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState("")
  const [error, setError] = useState<string | null>(null)
  const [device, setDevice] = useState<{ url: string; code: string } | null>(null)
  const [browserError, setBrowserError] = useState(false)
  const cancelled = useRef(false)

  async function openBrowser(url: string) {
    try {
      await openUrl(url)
      setBrowserError(false)
    } catch {
      setBrowserError(true)
    }
  }

  async function finishLocal() {
    setBusy(true)
    setError(null)
    try { await onComplete() }
    catch (cause) { setError(String(cause)) }
    finally { setBusy(false) }
  }

  async function connect(createAccount: boolean) {
    cancelled.current = false
    setBusy(true)
    setError(null)
    setBrowserError(false)
    setStatus("Preparing account sign-in…")
    let stop: (() => void) | undefined
    try {
      stop = await onCliProgress((progress) => {
        if (progress.job !== "library_login" || cancelled.current) return
        const event = progress.event
        if (event?.event !== "device_code" || typeof event.url !== "string" || typeof event.code !== "string") return
        const verification = new URL(event.url)
        // Registration returns to the device approval page with its code intact.
        const url = createAccount
          ? new URL(`/register?next=${encodeURIComponent(verification.pathname + verification.search)}`, verification.origin).href
          : verification.href
        setDevice({ url, code: event.code })
        setStatus("Finish in your browser, then approve this device. We’ll continue here automatically.")
        void openBrowser(url)
      })
      const installed = await cliStatus()
      if (cancelled.current) return
      if (!installed.found) {
        setStatus("Installing the local tools for account sign-in…")
        requireSuccess(await installTools(false))
      }
      if (cancelled.current) return
      setStatus("Connecting to Library…")
      requireSuccess(await runCli(null, { job: "library_login", server: LIBRARY }))
      if (cancelled.current) return
      await onComplete()
    } catch (cause) {
      if (!cancelled.current) setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      stop?.()
      setDevice(null)
      setStatus("")
      setBusy(false)
    }
  }

  return <main className="flex h-full overflow-y-auto bg-background px-6 py-8 text-foreground">
    <div className="m-auto w-full max-w-2xl space-y-7">
      <div className="space-y-2 text-center">
        <p className="text-xs font-medium uppercase tracking-widest text-muted-foreground">Welcome to Athenaeum</p>
        <h1 className="text-3xl font-semibold tracking-tight">Your knowledge, connected.</h1>
        <p className="text-sm text-muted-foreground">Turn your sources into a wiki you can explore, question, and grow.</p>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <section className="flex flex-col gap-4 rounded-xl border border-primary/30 bg-muted/30 p-5">
          <div className="flex items-center justify-between gap-2"><Cloud className="h-5 w-5" aria-hidden /><span className="rounded-full bg-primary/10 px-2 py-1 text-xs">Recommended</span></div>
          <div className="flex-1 space-y-2"><h2 className="font-semibold">Create an account</h2><p className="text-sm leading-relaxed text-muted-foreground">Keep your wiki local and use Library to sync accepted pages between computers when you choose.</p></div>
          <Button disabled={busy} onClick={() => void connect(true)}>Create an account</Button>
          <Button disabled={busy} variant="ghost" size="sm" onClick={() => void connect(false)}>Already have an account? Sign in</Button>
        </section>
        <section className="flex flex-col gap-4 rounded-xl border p-5">
          <HardDrive className="h-5 w-5" aria-hidden />
          <div className="flex-1 space-y-2"><h2 className="font-semibold">Start locally</h2><p className="text-sm leading-relaxed text-muted-foreground">Keep your wiki on this computer. No account needed. You can connect Library later.</p></div>
          <Button disabled={busy} variant="outline" onClick={() => void finishLocal()}>Continue locally</Button>
        </section>
      </div>
      {busy && <div role="status" className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="h-4 w-4 shrink-0 animate-spin" aria-hidden />{status || "Saving your choice…"}</div>}
      {device && <div className="space-y-3 rounded-lg border p-4 text-sm"><p>Confirm this code in your browser: <strong className="select-all font-mono tracking-widest">{device.code}</strong></p><Button variant="outline" onClick={() => void openBrowser(device.url)}>Open browser again</Button>{browserError && <p role="alert" className="break-all text-muted-foreground">Open this address in your browser: {device.url}</p>}</div>}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      {busy && status && <Button variant="ghost" onClick={() => {
        cancelled.current = true
        setStatus("Cancelling sign-in…")
        void cancelCli().catch((cause) => setError(String(cause)))
      }}>Cancel sign-in</Button>}
      <p className="text-center text-xs leading-relaxed text-muted-foreground">Library sync is optional. Your sources and local indexes stay on your computer.<br />AI features use the model you configure in Settings.</p>
    </div>
  </main>
}
