import { useEffect, useState } from "react"
import { Button } from "@/components/ui/button"
import { useWikiStore } from "@/stores/wiki-store"
import { useSetupStore } from "@/stores/setup-store"
import { normalizePath } from "@/lib/path-utils"
import { useAgentRuntimeStore } from "@/stores/agent-runtime-store"
import { openConnectors } from "@/stores/connectors-store"
import { ChatgptConnection } from "./chatgpt-connection"
import { BackgroundMemory } from "./background-memory"

const EXTERNAL_AGENTS = [
  { id: "claude", name: "Claude Code" },
  { id: "cursor", name: "Cursor" },
] as const

export function CodingAgentsView() {
  const project = useWikiStore((s) => s.project)
  return project ? <ProjectAgents key={normalizePath(project.path)} workspace={normalizePath(project.path)} /> : null
}

function ProjectAgents({ workspace }: { workspace: string }) {
  const { doctor, busy, error, check, install, setup } = useSetupStore()
  const [connecting, setConnecting] = useState<string | null>(null)
  const [connectionError, setConnectionError] = useState<string | null>(null)
  const working = busy !== null || connecting !== null
  useEffect(() => { void check(workspace) }, [workspace, check])

  async function connect(agent: "claude" | "cursor") {
    if (working) return
    setConnecting(agent)
    setConnectionError(null)
    try {
      const state = useSetupStore.getState()
      if ((!state.cli?.found || state.doctor?.checks.some((c) => c.id === "integrations" && !c.ok)) && !(await install(null))) return
      if (normalizePath(useWikiStore.getState().project?.path ?? "") !== workspace) return
      if (!(await setup(workspace, { job: "setup", agents: [agent], capture: "instruct" }))) {
        setConnectionError(useSetupStore.getState().error || "Could not connect this agent. Try again.")
      }
    } catch (cause) { setConnectionError(String(cause)) }
    finally { setConnecting(null) }
  }

  return <div className="space-y-8">
    <ChatgptConnection workspace={workspace} />
    <section className="space-y-3 border-t pt-6" aria-label="Model API">
      <h2 className="text-lg font-medium">Model API</h2>
      <div className="flex flex-wrap gap-3">
        <Button variant="outline" onClick={() => openConnectors("model")}>Configure model</Button>
        <Button variant="ghost" onClick={() => {
          useAgentRuntimeStore.getState().setRuntime("native")
          useWikiStore.getState().setActiveView("chat")
        }}>Use configured model</Button>
      </div>
    </section>
    <details className="border-t pt-6">
      <summary className="cursor-pointer text-sm font-medium">External agents</summary>
      <ul className="mt-4 divide-y">
        {EXTERNAL_AGENTS.map(({ id, name }) => {
          const connected = doctor?.checks.some((c) => c.id === `agent-${id}` && c.ok)
          return <li key={id} className="flex items-center justify-between gap-4 py-3 text-sm">
            <span>{name}{connected && <span role="status" className="ml-3 text-muted-foreground">Connected</span>}</span>
            <Button variant="outline" disabled={working} onClick={() => void connect(id)}>{connecting === id ? "Connecting…" : connected ? `Reconnect ${name}` : `Connect ${name}`}</Button>
          </li>
        })}
      </ul>
      {(connectionError || error) && <p role="alert" className="mt-3 text-sm text-destructive">{connectionError || error}</p>}
    </details>
    <details className="border-t pt-6">
      <summary className="cursor-pointer text-sm font-medium">Background memory</summary>
      <div className="mt-4"><BackgroundMemory workspace={workspace} /></div>
    </details>
  </div>
}
