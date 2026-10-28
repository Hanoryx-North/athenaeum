import { useRef, useState } from "react"
import { CheckCircle2, Loader2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { resolveTaskLlmConfig } from "@/lib/llm-task-routing"
import { testLlmFunction, type ProviderTestResult } from "@/lib/connection-tests"

export function ActiveModelStatus() {
  const state = useWikiStore()
  const config = resolveTaskLlmConfig("ingest", state.llmConfig, state.providerConfigs, state.taskModelRouting, state.projectLlmOverride, state.customLlmPresets)
  // Keep the config in memory only; it can contain credentials. A result belongs
  // to exactly the config tested and disappears as soon as that config changes.
  const identity = JSON.stringify(config)
  const currentIdentity = useRef(identity)
  currentIdentity.current = identity
  const [running, setRunning] = useState(false)
  const [tested, setTested] = useState<{ identity: string; result: ProviderTestResult } | null>(null)
  const result = !running && tested?.identity === identity ? tested.result : null
  const configured = config.provider === "codex-cli" || config.provider === "claude-code" || !!config.model.trim()

  async function verify() {
    setRunning(true)
    try {
      const result = await testLlmFunction(config)
      if (currentIdentity.current === identity) setTested({ identity, result })
    } catch (cause) {
      if (currentIdentity.current === identity) setTested({ identity, result: { ok: false, message: cause instanceof Error ? cause.message : String(cause) } })
    } finally { setRunning(false) }
  }

  return (
    <section className="space-y-3 rounded-xl border p-5" aria-label="Wiki model status">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div><p className="font-medium">Wiki model</p><p className="mt-1 text-sm text-muted-foreground">{configured ? `${config.provider} · ${config.model || "Default model"}` : "Choose a model below"}</p></div>
        <Button variant="outline" disabled={running || !configured} onClick={() => void verify()}>{running ? <Loader2 className="h-4 w-4 animate-spin" /> : result?.ok ? <CheckCircle2 className="h-4 w-4 text-emerald-600" /> : null}{running ? "Checking model…" : "Check model"}</Button>
      </div>
      <p className="text-sm text-muted-foreground">{running ? "Waiting for the model to respond…" : result ? result.message : "Not tested yet. Check model sends a short prompt to verify that it responds correctly."}</p>
      {result && <p role={result.ok ? "status" : "alert"} className={result.ok ? "text-sm text-emerald-600" : "text-sm text-destructive"}>{result.ok ? "Model responded successfully." : "Model check failed. Check your model settings and try again."}</p>}
      <p className="text-xs text-muted-foreground">This model turns sources into wiki pages. Coding agent connections use each agent’s own model and login.</p>
    </section>
  )
}
