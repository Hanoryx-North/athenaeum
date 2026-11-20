import { invoke } from "@tauri-apps/api/core"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"
import { useWikiStore } from "@/stores/wiki-store"

type Note = { kind: "titled"; title: string } | { kind: "daily"; date?: string }

async function openNote(note: Note): Promise<string> {
  const { project } = useWikiStore.getState()
  if (!project) throw new Error("Open a wiki before creating a note.")
  const relativePath = await invoke<string>("open_wiki_note", { projectPath: project.path, note })
  const path = `${project.path}/${relativePath}`
  const current = useWikiStore.getState().project
  if (current?.id === project.id && current.path === project.path) {
    useWikiStore.getState().openPathInPreview(path, "edit")
    await refreshProjectFileTree(project.path, { projectId: project.id, bumpDataVersion: true })
  }
  return path
}

export function createNote(title: string): Promise<string> {
  return openNote({ kind: "titled", title })
}

export function openDailyNote(date?: string): Promise<string> {
  return openNote({ kind: "daily", date })
}
