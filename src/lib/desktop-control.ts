import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { catalog, modules } from "virtual:desktop-catalog"
import { matchesArgument, decodeArgument, safeDesktopResult } from "@/lib/desktop-command-values"
import { GRAPH_LABEL_LIMITS } from "@/lib/graph-label"
import type { DesktopAction } from "@/types/desktop-control"

type Handler = (...args: unknown[]) => unknown
interface Registration { description: DesktopAction; run: Handler }
interface Request { id: string; operation: string; payload: { reveal?: boolean; request?: Record<string, unknown> } }
// Mounted surfaces and the native event listener outlive replaced Vite modules.
const registered: Map<string, Registration> = import.meta.hot?.data.registered ?? new Map()
if (import.meta.hot) import.meta.hot.data.registered = registered
const actions = new Map(catalog.entries.map(action => [action.name, action]))
const nativeNames = new Set(catalog.native.map(command => command.name))
const startedAt = new Date().toISOString()

/** UI composition registers its lifecycle operations here; no DOM or window automation. */
export function registerDesktopAction(name: string, signature: string, run: Handler): () => void {
  const entry: Registration = { description: { name, kind: "app", signature, parameters: [] }, run }
  registered.set(name, entry)
  return () => { if (registered.get(name) === entry) registered.delete(name) }
}

async function storeState(name: string): Promise<Record<string, unknown>> {
  const store = catalog.stores.find(store => store.name === name)
  if (!store) throw new Error(`Unknown store: ${name}`)
  const exports = await modules[store.module]()
  const api = exports[store.exportName] as { getState: () => Record<string, unknown> }
  return api.getState()
}

async function call(name: string, args: unknown[]): Promise<unknown> {
  const custom = registered.get(name)
  if (custom) return custom.run(...args)
  const action = actions.get(name)
  if (!action?.module || !action.exportName) throw new Error(`Unknown desktop action: ${name}. Discover actions with wiki desktop actions.`)
  if (args.length > action.parameters.length) throw new Error(`${name} expects ${action.signature}`)
  action.parameters.forEach((parameter, index) => {
    if (index >= args.length && parameter.optional) return
    if (index >= args.length || !matchesArgument(args[index], parameter.schema)) {
      throw new Error(`${name}: argument ${parameter.name} must be ${parameter.type}`)
    }
  })
  const exports = await modules[action.module]()
  const owner = action.kind === "store"
    ? await storeState(catalog.stores.find(s => s.module === action.module && s.exportName === action.exportName)!.name)
    : action.member ? exports[action.exportName] as Record<string, unknown> : exports
  const method = action.kind === "store" || action.member ? action.member! : action.exportName
  const handler = owner[method]
  if (typeof handler !== "function") throw new Error(`Action is unavailable: ${name}`)
  return Reflect.apply(handler, owner, args.map((value, index) => decodeArgument(value, action.parameters[index].schema)))
}

async function dispatch(operation: string, request: Record<string, unknown>): Promise<unknown> {
  switch (operation) {
    case "status": {
      const wiki = await storeState("wiki")
      return { schemaVersion: 1, frontendReady: true, startedAt, live: import.meta.env.DEV,
        project: wiki.project, activeView: wiki.activeView,
        actions: actions.size + registered.size, nativeCommands: nativeNames.size,
        graphLabelLimits: GRAPH_LABEL_LIMITS }
    }
    case "actions": {
      const filter = String(request.filter ?? "").toLowerCase()
      return {
        actions: [...catalog.entries, ...[...registered.values()].map(r => r.description)].filter(a => a.name.toLowerCase().includes(filter)),
        native: catalog.native.filter(a => a.name.toLowerCase().includes(filter)),
        stores: catalog.stores.map(store => store.name),
      }
    }
    case "state": {
      if (typeof request.store === "string") return storeState(request.store)
      return Object.fromEntries(await Promise.all(catalog.stores.map(async store => [store.name, await storeState(store.name)])))
    }
    case "call": {
      if (typeof request.action !== "string" || !Array.isArray(request.args)) throw new Error("Expected an action name and JSON argument array")
      return call(request.action, request.args)
    }
    case "invoke": {
      if (typeof request.command !== "string" || !nativeNames.has(request.command)) throw new Error("Unknown native desktop command")
      if (!request.args || typeof request.args !== "object" || Array.isArray(request.args)) throw new Error("Native arguments must be a JSON object")
      return invoke(request.command, request.args as Record<string, unknown>)
    }
    default: throw new Error(`Unknown desktop operation: ${operation}`)
  }
}

const transport: { dispatch: typeof dispatch } = import.meta.hot?.data.transport ?? { dispatch }
transport.dispatch = dispatch
if (import.meta.hot) import.meta.hot.data.transport = transport

export async function startDesktopControl(): Promise<() => void> {
  const session = crypto.randomUUID()
  const unlisten = await listen<Request>("desktop-control-request", async ({ payload }) => {
    let response: unknown
    try {
      const result = await transport.dispatch(payload.operation, payload.payload.request ?? {})
      response = { result: safeDesktopResult(result, payload.payload.reveal) }
    } catch (error) {
      response = { error: error instanceof Error ? error.message : String(error) }
    }
    await invoke("desktop_control_respond", { id: payload.id, response })
  })
  try { await invoke("desktop_control_ready", { session, ready: true }) }
  catch (error) { unlisten(); throw error }
  return () => {
    unlisten()
    void invoke("desktop_control_ready", { session, ready: false }).catch(error => console.error("Could not unregister desktop CLI listener:", error))
  }
}
