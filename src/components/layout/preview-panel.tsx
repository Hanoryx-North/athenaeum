import { useEffect, useCallback, useRef, useState } from "react"
import { X } from "lucide-react"
import { useWikiStore } from "@/stores/wiki-store"
import { readFile, writeFile } from "@/commands/fs"
import { getFileCategory, isBinary, isExtractedTextPreviewFile } from "@/lib/file-types"
import { WikiEditor } from "@/components/editor/wiki-editor"
import { FilePreview } from "@/components/editor/file-preview"
import { getFileName } from "@/lib/path-utils"
import { Button } from "@/components/ui/button"

const fileWrites = new Map<string, Promise<void>>()
const failedDrafts = new Map<string, { markdown: string; error: string }>()

export function PreviewPanel() {
  const selectedFile = useWikiStore((s) => s.selectedFile)
  const fileContent = useWikiStore((s) => s.fileContent)
  const previewContentPath = useWikiStore((s) => s.previewContentPath)
  const initialMode = useWikiStore((s) => s.previewInitialMode)
  const externalPreview = useWikiStore((s) => s.externalPreview)
  const setFileContent = useWikiStore((s) => s.setFileContent)
  const closePreview = useWikiStore((s) => s.closePreview)
  const [loadedPath, setLoadedPath] = useState<string | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [saveError, setSaveError] = useState<string | null>(null)
  const [retryRead, setRetryRead] = useState(0)
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const pendingSave = useRef<{ path: string; markdown: string } | null>(null)
  const lastSaved = useRef<{ path: string; markdown: string } | null>(null)

  useEffect(() => {
    setLoadError(null)
    setSaveError(null)
    if (!selectedFile) {
      setFileContent("")
      setLoadedPath(null)
      return
    }
    function showContent(content: string) {
      if (!selectedFile) return
      lastSaved.current = { path: selectedFile, markdown: content }
      const draft = failedDrafts.get(selectedFile)
      if (draft) {
        pendingSave.current = { path: selectedFile, markdown: draft.markdown }
        setSaveError(draft.error)
      }
      setFileContent(draft?.markdown ?? content)
      setLoadedPath(selectedFile)
    }

    if (previewContentPath === selectedFile || externalPreview?.path === selectedFile) {
      showContent(useWikiStore.getState().fileContent)
      return
    }

    const category = getFileCategory(selectedFile)

    if (isBinary(category) && !isExtractedTextPreviewFile(selectedFile)) {
      setFileContent("")
      setLoadedPath(selectedFile)
      return
    }

    let cancelled = false
    setLoadedPath(null)
    Promise.resolve(fileWrites.get(selectedFile)).catch(() => undefined)
      .then(() => readFile(selectedFile))
      .then(content => { if (!cancelled) showContent(content) })
      .catch((err) => {
        if (!cancelled) setLoadError(String(err))
      })
    return () => { cancelled = true }
  }, [selectedFile, previewContentPath, externalPreview, retryRead, setFileContent])

  const flushSave = useCallback(async () => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
    const change = pendingSave.current
    if (!change) return
    pendingSave.current = null
    const { path, markdown } = change
    const previousWrite = fileWrites.get(path)
    if (!previousWrite && !failedDrafts.has(path) && lastSaved.current?.path === path && lastSaved.current.markdown === markdown) return
    const write = Promise.resolve(previousWrite).catch(() => undefined).then(() => writeFile(path, markdown))
    fileWrites.set(path, write)
    try {
      await write
      if (fileWrites.get(path) === write) failedDrafts.delete(path)
      const wiki = useWikiStore.getState()
      if (wiki.selectedFile === path) {
        lastSaved.current = change
        wiki.setFileContent(markdown)
        setSaveError(null)
      }
      if (wiki.project && path.startsWith(`${wiki.project.path}/`)) wiki.bumpDataVersion()
    } catch (cause) {
      if (fileWrites.get(path) === write) {
        const error = String(cause)
        failedDrafts.set(path, { markdown, error })
        if (useWikiStore.getState().selectedFile === path) {
          pendingSave.current ??= change
          setSaveError(error)
        }
      }
      throw cause
    } finally {
      if (fileWrites.get(path) === write) fileWrites.delete(path)
    }
  }, [])

  const handleSave = useCallback((markdown: string, options?: { immediate?: boolean }) => {
    if (!selectedFile) return
    pendingSave.current = { path: selectedFile, markdown }
    if (options?.immediate) return flushSave()
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current)
    saveTimerRef.current = setTimeout(() => void flushSave().catch(() => undefined), 700)
  }, [selectedFile, flushSave])

  useEffect(() => {
    return () => { void flushSave().catch(cause => console.error("Could not save note:", cause)) }
  }, [selectedFile, flushSave])

  if (!selectedFile) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Select a note or open Log to create one.
      </div>
    )
  }

  const category = getFileCategory(selectedFile)
  const fileName = externalPreview?.path === selectedFile
    ? externalPreview.title
    : getFileName(selectedFile)

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b px-3 py-1.5">
        <span className="truncate text-xs text-muted-foreground" title={selectedFile}>
          {fileName}
        </span>
        <button
          onClick={closePreview}
          aria-label="Close note"
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-accent"
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
      {saveError && <div className="flex items-center gap-2 border-b px-3 py-2 text-sm">
        <p role="alert" className="min-w-0 break-words text-destructive">Could not save this note. {saveError}</p>
        <Button variant="outline" size="sm" onClick={() => void flushSave().catch(() => undefined)}>Retry</Button>
      </div>}
      <div className="flex-1 min-w-0 overflow-auto">
        {loadError ? <div className="space-y-2 p-4">
          <p role="alert" className="text-sm text-destructive">Could not open this note. {loadError}</p>
          <Button size="sm" variant="outline" onClick={() => setRetryRead(retryRead + 1)}>Retry</Button>
        </div> : loadedPath !== selectedFile ? <p role="status" className="p-4 text-sm text-muted-foreground">Loading…</p> : externalPreview?.path === selectedFile ? (
          <ExternalReferencePreview
            source={externalPreview.source}
            title={externalPreview.title}
            path={externalPreview.url}
            snippet={externalPreview.snippet || fileContent}
          />
        ) : category === "markdown" ? (
          <WikiEditor
            key={selectedFile}
            content={fileContent}
            onSave={handleSave}
            filePath={selectedFile}
            initialMode={saveError ? "edit" : initialMode}
          />
        ) : (
          <FilePreview
            key={selectedFile}
            filePath={selectedFile}
            textContent={fileContent}
          />
        )}
      </div>
    </div>
  )
}

function ExternalReferencePreview({
  source,
  title,
  path,
  snippet,
}: {
  source: string
  title: string
  path: string
  snippet: string
}) {
  return (
    <div className="flex h-full flex-col overflow-auto p-6">
      <div className="mb-4 space-y-2">
        <div className="flex items-center gap-2">
          <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase text-muted-foreground">
            {source}
          </span>
          <h3 className="truncate text-sm font-medium" title={title}>{title}</h3>
        </div>
        <div className="break-all rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          {path}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-border/60 bg-background p-4">
        <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-6">
          {snippet || "(No preview fragment returned.)"}
        </pre>
      </div>
    </div>
  )
}
