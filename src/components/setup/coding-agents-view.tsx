import { useEffect } from "react"
import { CheckCircle2, Loader2, Plus, RefreshCw } from "lucide-react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useSetupStore } from "@/stores/setup-store"
import { normalizePath } from "@/lib/path-utils"
import { useUiPreference } from "@/hooks/useUiPreference"
import { saveUiPreference } from "@/lib/ui-preferences"
import { AGENTS, SetupCard, type Agent } from "./setup-card"

export function CodingAgentsView() {
  const project = useWikiStore((s) => s.project)
  return project ? <ProjectAgents key={normalizePath(project.path)} workspace={normalizePath(project.path)} /> : null
}

function ProjectAgents({ workspace }: { workspace: string }) {
  const { doctor, busy, error, check } = useSetupStore()
  const [adding, setAdding] = useUiPreference(`agent-setup:${workspace}`, false, (value) => typeof value === "boolean")
  const connected = AGENTS.filter((agent) => doctor?.checks.some((c) => c.id === `agent-${agent.id}` && c.ok))
  const [agent, setAgent] = useUiPreference<Agent>(`setup-agent:${workspace}`, "athenaeum", (value) => AGENTS.some((a) => a.id === value))

  useEffect(() => { void check(workspace) }, [workspace, check])

  function finish() {
    saveUiPreference(`setup-step:${workspace}`, 0)
    setAdding(false)
  }

  if (adding) return <SetupCard initialAgent={agent} onAgentChange={setAgent} onComplete={finish} onCancel={finish} />

  return (
    <section className="space-y-6" aria-label="Coding agents">
      <header>
        <h2 className="text-2xl font-semibold">Coding agents</h2>
        <p className="mt-2 text-sm text-muted-foreground">Connect the coding agents you use to this project’s wiki. Athenaeum Agent uses your Settings model. External agents use their own accounts.</p>
      </header>
      {busy === "check" && <p role="status" className="flex items-center gap-2 text-sm text-muted-foreground"><Loader2 className="h-4 w-4 animate-spin" />Checking connections…</p>}
      {connected.length > 0 ? (
        <ul className="space-y-3" aria-label="Connected coding agents">
          {connected.map((item) => (
            <li key={item.id} className="flex flex-wrap items-center justify-between gap-4 rounded-xl border p-5">
              <div className="flex items-center gap-3">
                <CheckCircle2 className="h-5 w-5 text-emerald-600" />
                <div><p className="font-medium">{item.name}</p><p className="mt-1 text-sm text-muted-foreground">Connected to this wiki</p></div>
              </div>
              <Button variant="outline" disabled={busy !== null} onClick={() => { setAgent(item.id); saveUiPreference(`setup-step:${workspace}`, 1); setAdding(true) }}>Reconnect {item.name}</Button>
            </li>
          ))}
        </ul>
      ) : busy !== "check" && (
        <div className="rounded-xl border border-dashed p-6 text-sm text-muted-foreground">{doctor ? "No coding agents are connected yet. Add your first agent to start using wiki memory while you code." : "Connections have not been verified. Check again or add an agent."}</div>
      )}
      {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
      <div className="flex flex-wrap gap-3">
        <Button disabled={busy !== null} onClick={() => { saveUiPreference(`setup-step:${workspace}`, 0); setAdding(true) }}><Plus className="h-4 w-4" />Add agent</Button>
        <Button variant="ghost" disabled={busy !== null} onClick={() => void check(workspace)}><RefreshCw className="h-4 w-4" />Check connections</Button>
      </div>
      <p className="text-xs leading-relaxed text-muted-foreground">Connecting saves wiki tools and memory instructions for your agent. New findings appear in Review for you to approve.</p>
    </section>
  )
}
