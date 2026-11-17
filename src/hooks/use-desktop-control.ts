import { useEffect, useRef } from "react"
import { invoke } from "@tauri-apps/api/core"
import { createProject, openProject } from "@/commands/fs"
import { useWikiStore } from "@/stores/wiki-store"
import { useChatStore } from "@/stores/chat-store"
import { flushAndSuspendAutoSave } from "@/lib/auto-save"
import { registerDesktopAction, startDesktopControl } from "@/lib/desktop-control"
import { registerPlatformActions } from "@/lib/desktop-platform-actions"
import type { WikiProject } from "@/types/wiki"

function text(value: unknown, name: string): string {
  if (typeof value !== "string" || !value.trim()) throw new Error(`${name} must be a nonempty string`)
  return value
}

export function useDesktopControl(ready: boolean, onOpen: (project: WikiProject) => Promise<void>): void {
  const openRef = useRef(onOpen)
  openRef.current = onOpen
  useEffect(() => {
    if (!ready) return
    let disposed = false
    let stopListening: (() => void) | undefined
    const stop = [
      registerPlatformActions(),
      registerDesktopAction("project.open", "(path: string): Promise<WikiProject>", async path => {
        const project = await openProject(text(path, "path"))
        await openRef.current(project)
        return project
      }),
      registerDesktopAction("project.create", "(name: string, path?: string): Promise<WikiProject>", async (name, path) => {
        const project = await createProject(text(name, "name"), path === undefined ? undefined : text(path, "path"))
        await openRef.current(project)
        return project
      }),
      registerDesktopAction("app.refresh", "(): void", () => {
        useWikiStore.getState().bumpDataVersion()
      }),
      ...["reload", "quit"].map(operation => registerDesktopAction(`app.${operation}`, "(): Promise<void>", async () => {
        if (useChatStore.getState().isStreaming) throw new Error("A chat turn is active. Cancel it before reloading or quitting.")
        await flushAndSuspendAutoSave()
        if (operation === "quit") await invoke("desktop_control_quit")
        else window.setTimeout(() => window.location.reload(), 200)
      })),
    ]
    startDesktopControl().then(unlisten => {
      if (disposed) unlisten()
      else stopListening = unlisten
    }).catch(error => console.error("Desktop CLI transport could not start:", error))
    return () => { disposed = true; stopListening?.(); stop.forEach(unregister => unregister()) }
  }, [ready])
}
