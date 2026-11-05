import { useRef, useEffect, useCallback, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { convertFileSrc, invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { BookOpen, Plus, Trash2, MessageSquare, X, Maximize2, FolderOpen, FileText, ListTree, ChevronDown, ChevronRight, CircleAlert, CircleCheck, ClipboardCheck, LoaderCircle } from "lucide-react"
import { Button } from "@/components/ui/button"
import { ChatMessage, StreamingMessage, useSourceFiles, type ChatReferencePreview } from "./chat-message"
import { ChatInput, type ChatSendOptions, type ChatSkillOption } from "./chat-input"
import { useChatStore, type MessageImage, type MessageReference } from "@/stores/chat-store"
import { useWikiStore } from "@/stores/wiki-store"
import { registerDesktopAction } from "@/lib/desktop-control"
import { openConnectors } from "@/stores/connectors-store"
import { resolveTaskLlmConfig } from "@/lib/llm-task-routing"
import { harnessRouteFor, nativeRouteFor } from "@/lib/harness-route"
import { useUiPreference } from "@/hooks/useUiPreference"
import { supportsImageInput } from "@/lib/llm-providers"
import { executeIngestWrites } from "@/lib/ingest"
import { deleteFile, openPathInProject, readFile } from "@/commands/fs"
import { getFileName, isAbsolutePath, normalizePath } from "@/lib/path-utils"
import { hasConfiguredAnyTxt } from "@/lib/anytxt-search"
import type { ChatAgentEvent, ChatAgentStep, ChatUserInputRequest } from "@/lib/chat-agent-types"
import { FilePreview } from "@/components/editor/file-preview"
import { WikiReader } from "@/components/editor/wiki-reader"
import { FrontmatterPanel } from "@/components/editor/frontmatter-panel"
import { parseFrontmatter } from "@/lib/frontmatter"
import { getFileCategory, getFileExtension, isTextReadable } from "@/lib/file-types"
import { refreshProjectFileTree } from "@/lib/project-file-tree-refresh"
import { ReferenceKnowledgeGraph } from "@/components/chat/reference-knowledge-graph"
import {
  describeToolCall,
  describeToolResult,
  forgetHarnessSessions,
  harnessSessionFor,
  historyPreamble,
} from "@/lib/harness-chat"

type InternalChatSendOptions = ChatSendOptions & {
  suppressUserMessage?: boolean
  historyOverride?: { role: "user" | "assistant"; content: string }[]
}

interface BackendAgentToolEvent {
  tool: string
  status: string
  detail?: string
  timestamp?: number
}

interface HarnessNotificationPayload {
  runId: string
  notification: {
    method: string
    params: Record<string, unknown>
  }
}

interface HarnessStatus {
  workspace: string
  provider: string
  model: string
}

interface HarnessTurnResult {
  sessionId: string
  finalResponse: string
}

type ContextDetailCategory = "wiki" | "graph" | "web" | "anytxt" | "workspace" | "external"

interface ContextDetailItem extends MessageReference {
  content: string
  charCount: number
  category: ContextDetailCategory
}

function contextDetailCategory(reference: MessageReference): ContextDetailCategory {
  const source = reference.source?.trim().toLowerCase()
  if (source === "graph") return "graph"
  if (source === "anytxt") return "anytxt"
  if (source === "web") return "web"
  if (reference.kind === "workspace") return "workspace"
  if (reference.kind === "wiki") return "wiki"
  return "external"
}

// Store the page mapping from the last query so SourceFilesBar can show which pages were cited
export let lastQueryPages: { title: string; path: string }[] = []

function parentDirectory(path: string): string {
  const normalized = normalizePath(path).replace(/\/+$/g, "")
  const idx = normalized.lastIndexOf("/")
  if (idx <= 0) return normalized
  return normalized.slice(0, idx)
}

function commonDirectory(paths: string[]): string | null {
  const directories = paths
    .map(parentDirectory)
    .filter((dir) => dir.trim().length > 0)
  if (directories.length === 0) return null
  const firstParts = directories[0].split("/")
  let commonLength = firstParts.length
  for (const dir of directories.slice(1)) {
    const parts = dir.split("/")
    commonLength = Math.min(commonLength, parts.length)
    for (let i = 0; i < commonLength; i += 1) {
      if (firstParts[i] !== parts[i]) {
        commonLength = i
        break
      }
    }
  }
  return firstParts.slice(0, commonLength).join("/") || null
}

function formatDate(timestamp: number): string {
  const d = new Date(timestamp)
  const now = new Date()
  const isToday = d.toDateString() === now.toDateString()
  if (isToday) {
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
  }
  return d.toLocaleDateString([], { month: "short", day: "numeric" })
}

function ConversationSidebar({
  onNewConversation,
  onSelectConversation,
}: {
  onNewConversation?: () => void
  onSelectConversation?: (id: string) => void
}) {
  const { t } = useTranslation()
  const conversations = useChatStore((s) => s.conversations)
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const messages = useChatStore((s) => s.messages)
  const createConversation = useChatStore((s) => s.createConversation)
  const deleteConversation = useChatStore((s) => s.deleteConversation)
  const setActiveConversation = useChatStore((s) => s.setActiveConversation)

  const [hoveredId, setHoveredId] = useState<string | null>(null)

  const sorted = [...conversations].sort((a, b) => b.updatedAt - a.updatedAt)

  function getMessageCount(convId: string): number {
    return messages.filter((m) => m.conversationId === convId).length
  }

  return (
    <div className="flex h-full w-[200px] flex-shrink-0 flex-col border-r bg-muted/30">
      <div className="border-b p-2">
        <Button
          variant="outline"
          size="sm"
          className="w-full gap-2"
          onClick={() => {
            if (onNewConversation) {
              onNewConversation()
            } else {
              createConversation()
            }
          }}
        >
          <Plus className="h-3.5 w-3.5" />
          {t("chat.newChat")}
        </Button>
      </div>

      <div className="flex-1 overflow-y-auto py-1">
        {sorted.length === 0 ? (
          <p className="px-3 py-4 text-xs text-muted-foreground text-center">
            {t("chat.noConversationsYet")}
          </p>
        ) : (
          sorted.map((conv) => {
            const isActive = conv.id === activeConversationId
            const msgCount = getMessageCount(conv.id)
            return (
              <div
                key={conv.id}
                className={`group relative mx-1 my-0.5 flex cursor-pointer flex-col rounded-md px-2 py-1.5 text-sm transition-colors ${
                  isActive
                    ? "bg-primary/10 text-primary"
                    : "hover:bg-accent text-foreground"
                }`}
                onClick={() => {
                  if (onSelectConversation) {
                    onSelectConversation(conv.id)
                  } else {
                    setActiveConversation(conv.id)
                  }
                }}
                onMouseEnter={() => setHoveredId(conv.id)}
                onMouseLeave={() => setHoveredId(null)}
              >
                <div className="flex items-start justify-between gap-1">
                  <span className="line-clamp-2 flex-1 text-xs font-medium leading-snug">
                    {conv.title}
                  </span>
                  {hoveredId === conv.id && (
                    <button
                      className="flex-shrink-0 rounded p-0.5 text-muted-foreground hover:text-destructive"
                      onClick={(e) => {
                        e.stopPropagation()
                        deleteConversation(conv.id)
                        // Delete persisted chat file
                        const proj = useWikiStore.getState().project
                        if (proj) {
                          deleteFile(`${proj.path}/.athenaeum/chats/${conv.id}.json`).catch(() => {})
                        }
                      }}
                    >
                      <Trash2 className="h-3 w-3" />
                    </button>
                  )}
                </div>
                <div className="mt-0.5 flex items-center gap-1.5 text-[10px] text-muted-foreground">
                  <span>{formatDate(conv.updatedAt)}</span>
                  {msgCount > 0 && (
                    <>
                      <span>·</span>
                      <span>{msgCount} {t("chat.msgCount")}</span>
                    </>
                  )}
                </div>
              </div>
            )
          })
        )}
      </div>
    </div>
  )
}

function projectAbsolutePath(projectPath: string, path: string): string {
  const pp = normalizePath(projectPath)
  const normalized = normalizePath(path)
  if (normalized.startsWith(`${pp}/`)) return normalized
  if (isAbsolutePath(normalized)) return normalized
  return `${pp}/${normalized.replace(/^\/+/, "")}`
}

function isAgentWorkspacePath(filePath: string): boolean {
  return normalizePath(filePath).split("/").includes("agent-workspace")
}

function isGeneratedOutputImage(filePath: string): boolean {
  const category = getFileCategory(filePath)
  return category === "image" || (getFileExtension(filePath) === "svg" && isAgentWorkspacePath(filePath))
}

function backendToolToAgentStep(event: BackendAgentToolEvent, index: number) {
  if (event.tool === "agent.plan_tools") {
    return {
      id: `backend-${index}-${event.tool}-${event.status}`,
      type: "routing" as const,
      message: event.detail ?? event.tool,
      status: event.status === "failed" ? "error" as const : "success" as const,
      timestamp: event.timestamp,
    }
  }
  if (event.tool === "llm.generate") {
    return {
      id: `backend-${index}-${event.tool}-${event.status}`,
      type: "final" as const,
      message: event.detail ?? event.tool,
      status: event.status === "failed" ? "error" as const
        : event.status === "started" ? "running" as const
          : "success" as const,
      timestamp: event.timestamp,
    }
  }
  const tool = normalizeBackendToolName(event.tool)
  return {
    id: `backend-${index}-${event.tool}-${event.status}`,
    type: event.status === "started" ? "tool_call" as const : "tool_result" as const,
    tool,
    message: event.detail ?? event.tool,
    status: event.status === "failed" ? "error" as const
      : event.status === "available" ? "skipped" as const
        : event.status === "started" ? "running" as const
          : "success" as const,
    timestamp: event.timestamp,
  }
}

function normalizeBackendToolName(tool: string) {
  const normalized = tool.split(".").join("_")
  if (normalized === "wiki_search") return "wiki_search" as const
  if (normalized === "wiki_read_page") return "project_file_read" as const
  if (normalized === "wiki_write_page") return "project_files" as const
  if (normalized === "workspace_write_file") return "project_files" as const
  if (normalized === "workspace_append_file") return "project_files" as const
  if (normalized === "skills_load") return "project_file_read" as const
  if (normalized === "context_attach") return "project_file_read" as const
  if (normalized === "skill_read_file") return "project_file_read" as const
  if (normalized === "source_search") return "project_file_read" as const
  if (normalized === "graph_search") return "graph_search" as const
  if (normalized === "web_search") return "web_search" as const
  if (normalized === "anytxt_search") return "anytxt_search" as const
  if (normalized === "shell_exec") return "shell_exec" as const
  if (normalized === "deep_research_run") return "project_file_read" as const
  // DeepSeek Harness tools, as `tool/call` names them.
  if (normalized === "pwsh" || normalized === "bash") return "shell_exec" as const
  if (normalized === "write" || normalized === "edit") return "project_files" as const
  if (["read", "read_image", "glob", "grep", "skill", "wiki_open", "wiki_evidence", "wiki_history"].includes(normalized)) {
    return "project_file_read" as const
  }
  if (normalized === "wiki_navigate" || normalized === "wiki_related") return "graph_search" as const
  if (normalized === "web_fetch") return "web_search" as const
  return "unknown_tool" as const
}

function backendToolToAgentEvent(event: BackendAgentToolEvent): ChatAgentEvent {
  if (event.tool === "agent.plan_tools") {
    return {
      stage: "routing",
      message: event.detail ?? event.tool,
      status: event.status === "failed" ? "error" : "success",
      timestamp: event.timestamp,
    }
  }
  if (event.tool === "llm.generate") {
    return {
      stage: "writing",
      message: event.detail ?? event.tool,
      status: event.status === "failed" ? "error"
        : event.status === "started" ? "running"
          : "success",
      timestamp: event.timestamp,
    }
  }
  const tool = normalizeBackendToolName(event.tool)
  const stage =
    tool === "web_search" ? "searching_web"
      : tool === "anytxt_search" ? "searching_anytxt"
        : tool === "graph_search" ? "searching_graph"
          : tool === "project_file_read" ? "reading_context"
            : tool === "wiki_search" ? "searching_wiki"
              : event.status === "started" ? "tool_call"
                : "tool_result"
  return {
    stage,
    tool,
    message: event.detail ?? event.tool,
    status: event.status === "failed" ? "error"
      : event.status === "started" ? "running"
        : event.status === "available" ? "skipped"
          : "success",
    timestamp: event.timestamp,
  }
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null
}

function textFromContent(value: unknown): string {
  if (!Array.isArray(value)) return ""
  return value.flatMap((block) => {
    const item = asRecord(block)
    return item?.type === "text" && typeof item.text === "string" ? [item.text] : []
  }).join("")
}

function summarizeAgentStepsForResume(steps: ChatAgentStep[] = []): string {
  const lines = steps
    .filter((step) => step.message?.trim())
    .slice(-12)
    .map((step) => {
      const label = step.tool ?? step.type
      const status = step.status ?? "success"
      return `- ${label} ${status}: ${step.message?.trim()}`
    })
  return lines.length > 0 ? lines.join("\n") : "- No prior tool observations were saved."
}

function compactChatHistoryForResume(
  messages: Array<{ role: "user" | "assistant" | "system"; content: string }>,
  maxMessages: number,
): { role: "user" | "assistant"; content: string }[] {
  return messages
    .filter((message): message is { role: "user" | "assistant"; content: string } =>
      message.role === "user" || message.role === "assistant"
    )
    .slice(-maxMessages)
    .map((message) => ({
      role: message.role,
      content: message.content,
    }))
}

export function ChatPanel() {
  const { t } = useTranslation()
  useSourceFiles() // Keep source file cache warm
  const activeConversationId = useChatStore((s) => s.activeConversationId)
  const isStreaming = useChatStore((s) => s.isStreaming)
  const streamingContent = useChatStore((s) => s.streamingContent)
  const mode = useChatStore((s) => s.mode)
  const addMessageToConversation = useChatStore((s) => s.addMessageToConversation)
  const setStreaming = useChatStore((s) => s.setStreaming)
  const appendStreamToken = useChatStore((s) => s.appendStreamToken)
  const finalizeStreamForConversation = useChatStore((s) => s.finalizeStreamForConversation)
  const createConversation = useChatStore((s) => s.createConversation)
  const removeLastAssistantMessage = useChatStore((s) => s.removeLastAssistantMessage)
  const maxHistoryMessages = useChatStore((s) => s.maxHistoryMessages)
  const useWebSearch = useChatStore((s) => s.useWebSearch)
  const useAnyTxtSearch = useChatStore((s) => s.useAnyTxtSearch)
  const agentMode = useChatStore((s) => s.agentMode)
  const retrievalMode = useChatStore((s) => s.retrievalMode)
  const selectedSkills = useChatStore((s) => s.selectedSkills)
  const selectedContextFiles = useChatStore((s) => s.selectedContextFiles)
  const setUseWebSearch = useChatStore((s) => s.setUseWebSearch)
  const setUseAnyTxtSearch = useChatStore((s) => s.setUseAnyTxtSearch)
  const setAgentMode = useChatStore((s) => s.setAgentMode)
  const setRetrievalMode = useChatStore((s) => s.setRetrievalMode)
  const setSelectedSkills = useChatStore((s) => s.setSelectedSkills)
  const setSelectedContextFiles = useChatStore((s) => s.setSelectedContextFiles)

  // Derive active messages via selector to re-render on message changes
  const allMessages = useChatStore((s) => s.messages)
  const activeMessages = activeConversationId
    ? allMessages.filter((m) => m.conversationId === activeConversationId)
    : []

  const project = useWikiStore((s) => s.project)
  const setActiveView = useWikiStore((s) => s.setActiveView)
  const projectPathIndex = useWikiStore((s) => s.projectPathIndex)
  const baseLlmConfig = useWikiStore((s) => s.llmConfig)
  const providerConfigs = useWikiStore((s) => s.providerConfigs)
  const taskModelRouting = useWikiStore((s) => s.taskModelRouting)
  const projectLlmOverride = useWikiStore((s) => s.projectLlmOverride)
  const customLlmPresets = useWikiStore((s) => s.customLlmPresets)
  const llmConfig = useMemo(
    () => resolveTaskLlmConfig("chat", baseLlmConfig, providerConfigs, taskModelRouting, projectLlmOverride, customLlmPresets),
    [baseLlmConfig, providerConfigs, taskModelRouting, projectLlmOverride, customLlmPresets],
  )
  const searchApiConfig = useWikiStore((s) => s.searchApiConfig)
  const anyTxtAvailable = hasConfiguredAnyTxt(searchApiConfig.anyTxt)
  // The chat agent follows the provider chosen in Settings for chat.
  const [agentRuntime, setAgentRuntime] = useUiPreference<"native" | "harness">("chat-agent-runtime", "native", (value) => value === "native" || value === "harness")
  const [approval, setApproval] = useState<{ id: string; title: string; detail: string } | null>(null)
  const [approvalError, setApprovalError] = useState<string | null>(null)
  const [approvalBusy, setApprovalBusy] = useState(false)
  const chatRoute = useMemo(() => agentRuntime === "native" ? nativeRouteFor(llmConfig) : harnessRouteFor(llmConfig), [llmConfig, agentRuntime])
  // A new route restarts the runtime, and a restarted runtime refuses the session ids
  // the old one held, so every conversation starts afresh (carrying its history).
  const chatRouteKey = chatRoute.ok ? JSON.stringify(chatRoute.route) : ""
  const lastChatRouteKey = useRef(chatRouteKey)
  useEffect(() => {
    if (lastChatRouteKey.current === chatRouteKey) return
    lastChatRouteKey.current = chatRouteKey
    forgetHarnessSessions()
  }, [chatRouteKey])
  const imageInputAvailable = mode === "chat" || supportsImageInput(llmConfig)
  const availableContextFiles = useMemo(() => {
    if (!project) return []
    const root = normalizePath(project.path).replace(/\/+$/g, "")
    const files = [...projectPathIndex.filesByName.values()].flat()
    return [...new Set(files
      .map((entry) => normalizePath(entry.path))
      .filter((path) => path.startsWith(`${root}/`))
      .map((path) => path.slice(root.length + 1))
      .filter((path) => !path.split("/").some((segment) => segment.startsWith("."))))]
      .sort((left, right) => left.localeCompare(right))
  }, [project, projectPathIndex])

  const abortRef = useRef<AbortController | null>(null)
  const activeRunSessionIdRef = useRef<string | null>(null)
  const activeRunIdRef = useRef<string | null>(null)
  const runIdRef = useRef(0)
  const dismissedGeneratedOutputsKeyRef = useRef<string | null>(null)
  const scrollContainerRef = useRef<HTMLDivElement>(null)
  const bottomRef = useRef<HTMLDivElement>(null)
  const [agentEvents, setAgentEvents] = useState<ChatAgentEvent[]>([])
  const [referencePreview, setReferencePreview] = useState<ChatReferencePreview | null>(null)
  const [contextDetailReferences, setContextDetailReferences] = useState<MessageReference[] | null>(null)
  const [generatedOutputPreviews, setGeneratedOutputPreviews] = useState<ChatReferencePreview[]>([])
  const [generatedOutputPreview, setGeneratedOutputPreview] = useState<ChatReferencePreview | null>(null)
  const [referencePreviewWidth, setReferencePreviewWidth] = useState(420)
  const handleOpenContextDetails = useCallback((references: MessageReference[]) => {
    setReferencePreview(null)
    setGeneratedOutputPreviews([])
    setContextDetailReferences(references)
  }, [])
  const [availableSkills] = useState<ChatSkillOption[]>([])
  const [harnessStatus, setHarnessStatus] = useState<
    | { state: "checking" }
    | { state: "ready"; provider: string; model: string }
    | { state: "offline"; message: string }
  >({ state: "checking" })
  const [approvingShellMessageId, setApprovingShellMessageId] = useState<string | null>(null)
  const [streamingConversationId, setStreamingConversationId] = useState<string | null>(null)
  useEffect(() => {
    setContextDetailReferences(null)
  }, [activeConversationId])
  const activeStreaming = Boolean(isStreaming && activeConversationId && streamingConversationId === activeConversationId)
  const activeAgentEvents = activeStreaming ? agentEvents : []
  const lastMessage = activeMessages[activeMessages.length - 1]
  const latestGeneratedOutputMessage = [...activeMessages]
    .reverse()
    .find((message) =>
      message.role === "assistant"
      && (message.references ?? []).some((ref) => ref.kind === "workspace")
    )
  const scrollKey = [
    activeConversationId ?? "",
    activeMessages.length,
    lastMessage?.id ?? "",
    lastMessage?.content.length ?? 0,
    activeStreaming ? streamingContent.length : 0,
  ].join(":")

  // Auto-scroll to bottom when messages change or streaming content updates
  useEffect(() => {
    const container = scrollContainerRef.current
    if (container) {
      container.scrollTop = container.scrollHeight
    }
  }, [scrollKey])

  useEffect(() => {
    setReferencePreview(null)
    setGeneratedOutputPreviews([])
    setGeneratedOutputPreview(null)
    dismissedGeneratedOutputsKeyRef.current = null
  }, [activeConversationId])

  useEffect(() => {
    if (!project || activeStreaming || !latestGeneratedOutputMessage) return
    const outputs = (latestGeneratedOutputMessage.references ?? []).filter((ref) => ref.kind === "workspace")
    if (outputs.length === 0) return
    const previews = outputs.map((ref) => {
      const outputPath = projectAbsolutePath(project.path, ref.path)
      return {
        title: ref.title || getFileName(outputPath),
        path: outputPath,
        source: ref.source ?? "Workspace",
        content: "",
        snippet: ref.snippet,
      }
    })
    const currentKey = generatedOutputPreviews.map((preview) => preview.path).join("\n")
    const nextKey = previews.map((preview) => preview.path).join("\n")
    const scopedNextKey = `${activeConversationId ?? ""}:${nextKey}`
    if (dismissedGeneratedOutputsKeyRef.current === scopedNextKey) return
    if (currentKey === nextKey) return
    setReferencePreview(null)
    setGeneratedOutputPreviews(previews)
  }, [activeConversationId, activeStreaming, generatedOutputPreviews, latestGeneratedOutputMessage, project])

  const loadGeneratedOutputPreview = useCallback(async (preview: ChatReferencePreview): Promise<ChatReferencePreview> => {
    const category = getFileCategory(preview.path)
    const shouldReadContent = isTextReadable(category) || category === "pdf"
    if (!shouldReadContent || preview.content) return preview
    try {
      return {
        ...preview,
        content: await readFile(preview.path),
      }
    } catch {
      return preview
    }
  }, [])

  const openGeneratedOutputModal = useCallback((preview: ChatReferencePreview) => {
    void loadGeneratedOutputPreview(preview).then(setGeneratedOutputPreview)
  }, [loadGeneratedOutputPreview])

  const closeGeneratedOutputsPanel = useCallback(() => {
    const currentKey = generatedOutputPreviews.map((preview) => preview.path).join("\n")
    dismissedGeneratedOutputsKeyRef.current = `${activeConversationId ?? ""}:${currentKey}`
    setGeneratedOutputPreviews([])
    setGeneratedOutputPreview(null)
  }, [activeConversationId, generatedOutputPreviews])

  const openGeneratedOutputDirectory = useCallback(() => {
    if (!project) return
    const directory = commonDirectory(generatedOutputPreviews.map((preview) => preview.path))
    if (!directory) return
    void openPathInProject(project.path, directory).catch((err) => {
      console.error("[chat] failed to open generated output directory:", err)
    })
  }, [generatedOutputPreviews, project])

  const handleOpenReferencePreview = useCallback((preview: ChatReferencePreview, relatedPreviews?: ChatReferencePreview[]) => {
    setContextDetailReferences(null)
    const isGeneratedOutput = preview.source === "Workspace"
      || normalizePath(preview.path).split("/").includes("agent-workspace")
    if (!isGeneratedOutput) {
      setGeneratedOutputPreviews([])
      setGeneratedOutputPreview(null)
      setReferencePreview(preview)
      return
    }
    const previews = relatedPreviews && relatedPreviews.length > 0
      ? relatedPreviews.map((item) =>
          item.path === preview.path
            ? { ...item, content: preview.content }
            : item
        )
      : [preview]
    setReferencePreview(null)
    setGeneratedOutputPreviews(previews)
    openGeneratedOutputModal(preview)
  }, [openGeneratedOutputModal])

  useEffect(() => {
    let cancelled = false
    if (mode !== "chat") return
    if (!project?.path) {
      setHarnessStatus({ state: "offline", message: t("chat.agent.noWorkspace") })
      return
    }
    if (!chatRoute.ok) {
      setHarnessStatus({ state: "offline", message: chatRoute.reason })
      return
    }
    setHarnessStatus({ state: "checking" })
    invoke<HarnessStatus>("harness_check", { projectPath: project.path, route: chatRoute.route })
      .then((status) => {
        if (!cancelled) setHarnessStatus({ state: "ready", provider: status.provider, model: status.model })
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setHarnessStatus({
            state: "offline",
            message: error instanceof Error ? error.message : String(error),
          })
        }
      })
    return () => { cancelled = true }
  }, [mode, project?.path, chatRoute, t])

  const handleSend = useCallback(
    async (
      text: string,
      images: MessageImage[] = [],
      options?: InternalChatSendOptions,
    ) => {
      const sendOptions = options ?? {
        useWebSearch: useChatStore.getState().useWebSearch,
        useAnyTxtSearch: useChatStore.getState().useAnyTxtSearch,
        agentMode: useChatStore.getState().agentMode,
        retrievalMode: useChatStore.getState().retrievalMode,
        skills: useChatStore.getState().selectedSkills,
        contextFiles: useChatStore.getState().selectedContextFiles,
        skillMode: useChatStore.getState().selectedSkills.length > 0 ? "explicit" : "auto",
      }
      // Auto-create a conversation if none is active
      let convId = useChatStore.getState().activeConversationId
      if (!convId) {
        convId = createConversation()
      }

      if (!sendOptions.suppressUserMessage) {
        const messageContextFiles = project
          ? sendOptions.contextFiles.map((path) => projectAbsolutePath(project.path, path))
          : []
        addMessageToConversation(convId, "user", text, images, messageContextFiles)
      }
      setStreamingConversationId(convId)
      setStreaming(true)
      setAgentEvents([])
      let finalized = false
      const runId = ++runIdRef.current
      const backendRunId = `ui-${Date.now()}-${runId}`

      try {
        const controller = new AbortController()
        abortRef.current = controller
        activeRunSessionIdRef.current = convId
        activeRunIdRef.current = backendRunId
        const isCurrentRun = () => runIdRef.current === runId && !controller.signal.aborted

        if (!project?.path) {
          throw new Error(t("chat.agent.noWorkspace"))
        }
        if (!chatRoute.ok) throw new Error(chatRoute.reason)

        setAgentEvents([{
          stage: "understanding",
          status: "running",
          message: t("chat.agent.connecting"),
        }])
        // The harness keeps the conversation while its runtime lives; a conversation
        // the current runtime has not seen starts a fresh session with its history.
        const { sessionId: harnessSessionId, fresh } = harnessSessionFor(project.path, convId)
        const priorMessages = useChatStore.getState().messages
          .filter((m) => m.conversationId === convId && (m.role === "user" || m.role === "assistant"))
        const history = sendOptions.historyOverride
          ?? (sendOptions.suppressUserMessage ? priorMessages : priorMessages.slice(0, -1))
            .slice(-maxHistoryMessages)
            .map((m) => ({ role: m.role as "user" | "assistant", content: m.content }))
        const preamble = fresh ? historyPreamble(history) : ""
        let turnFailure: string | null = null
        let streamedNative = false
        const backendEvents: BackendAgentToolEvent[] = []
        const toolsByCallId = new Map<string, string>()
        const addAgentActivity = (activity: ChatAgentEvent) => {
          setAgentEvents((previous) => [...previous, activity].slice(-6))
        }
        const unlisten = await listen<HarnessNotificationPayload>("harness-notification", (event) => {
          const payload = event.payload
          if (payload.runId !== backendRunId || !isCurrentRun()) return
          const notification = payload.notification
          const params = notification.params
          if (notification.method === "session.status") {
            if (params.sessionId !== harnessSessionId) return
            if (params.status === "running") {
              addAgentActivity({
                stage: "understanding",
                status: "running",
                message: t("chat.agent.running"),
                timestamp: Date.now(),
              })
            }
            return
          }
          if (notification.method !== "session.event" || params.sessionId !== harnessSessionId) return
          const agentEvent = asRecord(params.event)
          if (!agentEvent || typeof agentEvent.type !== "string") return
          const data = asRecord(agentEvent.data) ?? {}
          if (agentEvent.type === "message/delta" && typeof data.text === "string") {
            streamedNative = true
            appendStreamToken(data.text)
            return
          }
          if (agentEvent.type === "approval/request" && typeof data.id === "string") {
            setApproval({ id: data.id, title: String(data.title ?? "Agent approval"), detail: String(data.detail ?? "") })
            setApprovalError(null)
            return
          }

          if (agentEvent.type === "step/start") {
            addAgentActivity({
              stage: "understanding",
              status: "running",
              message: t("chat.agent.running"),
              timestamp: Date.now(),
            })
            return
          }
          if (agentEvent.type === "tool/call") {
            const name = typeof data.name === "string" ? data.name : "tool"
            const callId = typeof data.callId === "string" ? data.callId : ""
            if (callId) toolsByCallId.set(callId, name)
            const detail = typeof data.arguments === "string" ? data.arguments : ""
            const toolEvent: BackendAgentToolEvent = {
              tool: name,
              status: "started",
              detail: describeToolCall(name, detail),
              timestamp: Date.now(),
            }
            backendEvents.push(toolEvent)
            setAgentEvents((prev) => [...prev, backendToolToAgentEvent(toolEvent)].slice(-6))
            return
          }
          if (agentEvent.type === "tool/result") {
            const message = asRecord(data.message)
            const callId = typeof message?.toolCallId === "string"
              ? message.toolCallId
              : typeof data.callId === "string" ? data.callId : ""
            const name = (callId && toolsByCallId.get(callId)) || "tool"
            const failed = message?.isError === true || asRecord(data.error) !== null
            const toolEvent: BackendAgentToolEvent = {
              tool: name,
              status: failed ? "failed" : "completed",
              detail: describeToolResult(name, textFromContent(message?.content), failed),
              timestamp: Date.now(),
            }
            backendEvents.push(toolEvent)
            setAgentEvents((prev) => [...prev, backendToolToAgentEvent(toolEvent)].slice(-6))
            return
          }
          if (agentEvent.type === "turn/end") {
            const reason = asRecord(data.reason)
            if (reason?.kind === "error") {
              const error = asRecord(reason.error)
              turnFailure = typeof error?.message === "string"
                ? error.message
                : t("chat.agent.turnFailed")
            addAgentActivity({
                stage: "tool_result",
                status: "error",
                message: turnFailure ?? t("chat.agent.turnFailed"),
                timestamp: Date.now(),
            })
            }
          }
        })

        try {
          const selectedFiles = sendOptions.contextFiles
          const promptText = selectedFiles.length > 0
          ? `${text}\n\nPlease inspect these user-selected workspace files first:\n${selectedFiles.map((path) => `- ${path}`).join("\n")}`
            : text
          const contentBlocks = [
            { type: "text", text: preamble + promptText },
            ...images.map((image) => ({
              type: "image",
              data: image.dataBase64,
              mimeType: image.mediaType,
            })),
          ]
          let response: HarnessTurnResult
          try {
            response = await invoke<HarnessTurnResult>("harness_start_turn_stream", {
              projectPath: project.path,
              sessionId: harnessSessionId,
              runId: backendRunId,
              contentBlocks,
              route: chatRoute.route,
            })
          } catch (err) {
            // A failed run may have taken the runtime with it (a driver crash, a
            // stopped turn); start every conversation afresh rather than reuse an id
            // a new runtime would refuse.
            forgetHarnessSessions()
            throw err
          }
          if (!isCurrentRun()) return
          if (!response.finalResponse.trim() && turnFailure) throw new Error(turnFailure)
          const steps = backendEvents.map(backendToolToAgentStep)
          if (response.finalResponse && !streamedNative) {
            appendStreamToken(response.finalResponse)
          }
          finalized = true
          finalizeStreamForConversation(convId, response.finalResponse, [], steps)
          lastQueryPages = []
          setAgentEvents([])
          setStreamingConversationId(null)
          abortRef.current = null
          activeRunSessionIdRef.current = null
          activeRunIdRef.current = null
        } finally {
          setApproval(null)
          unlisten()
        }
        return
      } catch (err) {
        if (!finalized) {
          if (isAbortLikeError(err) || runIdRef.current !== runId) {
            setStreaming(false)
            setAgentEvents([])
            setStreamingConversationId(null)
            abortRef.current = null
            activeRunSessionIdRef.current = null
            activeRunIdRef.current = null
            return
          }
          const message = err instanceof Error ? err.message : String(err)
          finalizeStreamForConversation(convId, `Error: ${message}`, undefined)
          setAgentEvents([])
          setStreamingConversationId(null)
        }
        abortRef.current = null
        activeRunSessionIdRef.current = null
        activeRunIdRef.current = null
      }
    },
    [project, addMessageToConversation, setStreaming, appendStreamToken, finalizeStreamForConversation, createConversation, maxHistoryMessages, chatRoute, t],
  )

  const handleStop = useCallback(() => {
    setApproval(null)
    runIdRef.current += 1
    const sessionId = activeRunSessionIdRef.current
    const backendRunId = activeRunIdRef.current
    if (sessionId) {
      // Cancelling closes the runtime, and its sessions with it.
      forgetHarnessSessions()
      void invoke("harness_cancel_turn", {
        runId: backendRunId ?? "",
      }).catch(() => {})
    }
    abortRef.current?.abort()
    abortRef.current = null
    activeRunSessionIdRef.current = null
    activeRunIdRef.current = null
    setStreaming(false)
    setAgentEvents([])
    setStreamingConversationId(null)
  }, [setStreaming])

  const handleNewConversation = useCallback(() => {
    handleStop()
    setReferencePreview(null)
    setGeneratedOutputPreviews([])
    setGeneratedOutputPreview(null)
    setApprovingShellMessageId(null)
    dismissedGeneratedOutputsKeyRef.current = null
    createConversation()
  }, [createConversation, handleStop])

  const handleSelectConversation = useCallback((conversationId: string) => {
    useChatStore.getState().setActiveConversation(conversationId)
    setApprovingShellMessageId(null)
  }, [])

  const handleRegenerate = useCallback(async () => {
    if (activeStreaming) return
    // Find the last user message in active conversation
    const active = useChatStore.getState().getActiveMessages()
    const lastUserMsg = [...active].reverse().find((m) => m.role === "user")
    if (!lastUserMsg) return
    // Remove the last assistant reply, then re-send
    removeLastAssistantMessage()
    // Small delay to let state update
    await new Promise((r) => setTimeout(r, 50))
    // Trigger send with the same text (handleSend will add a new user message,
    // so also remove the original to avoid duplication)
    // Actually: just call handleSend — but it adds a user message. To avoid dupe,
    // we remove the last user message too and let handleSend re-add it.
    const store = useChatStore.getState()
    const updatedActive = store.getActiveMessages()
    const lastUser = [...updatedActive].reverse().find((m) => m.role === "user")
    if (lastUser) {
      const activeId = useChatStore.getState().activeConversationId
      useChatStore.setState((s) => ({
        messages: s.messages.filter((m) => m.conversationId !== activeId || m.id !== lastUser.id),
      }))
    }
    // Re-send with the original text AND images so a regenerated turn
    // keeps the same vision context.
    handleSend(lastUserMsg.content, lastUserMsg.images ?? [])
  }, [activeStreaming, removeLastAssistantMessage, handleSend])

  const handleApproveShellCommand = useCallback(async (command: string, assistantMessageId: string) => {
    if (!command.trim() || approvingShellMessageId) return
    const active = useChatStore.getState().getActiveMessages()
    const assistantIndex = active.findIndex((message) => message.id === assistantMessageId)
    if (assistantIndex <= 0) {
      console.warn("[chat] shell approval ignored: assistant message not found", assistantMessageId)
      return
    }
    const priorUser = [...active.slice(0, assistantIndex)]
      .reverse()
      .find((message) => message.role === "user")
    if (!priorUser) {
      console.warn("[chat] shell approval ignored: no prior user message")
      return
    }
    const assistantMessage = active[assistantIndex]
    const resumeHistory = [
      ...compactChatHistoryForResume(active.slice(0, assistantIndex), maxHistoryMessages),
      {
        role: "assistant" as const,
        content: [
          "The previous Agent turn stopped at a shell approval boundary.",
          "Preserved tool progress before approval:",
          summarizeAgentStepsForResume(assistantMessage.agentSteps),
          "",
          assistantMessage.content,
        ].join("\n"),
      },
    ]
    const resumeMessage = [
      "Continue the same Agent task from the preserved tool progress. The user approved the pending shell command; execute only that approved command first, then continue from its result. Do not restart completed setup, file reads, or workspace writes unless the command result proves they are invalid.",
    ].join("\n")
    setApprovingShellMessageId(assistantMessageId)
    // Approval is a continuation of a turn that has already stopped at a
    // permission boundary. Clear any stale streaming state before resuming so a
    // delayed store update cannot make the button feel inert.
    abortRef.current?.abort()
    abortRef.current = null
    activeRunSessionIdRef.current = null
    activeRunIdRef.current = null
    setStreaming(false)
    try {
      await handleSend(resumeMessage, priorUser.images ?? [], {
        useWebSearch: useChatStore.getState().useWebSearch,
        useAnyTxtSearch: useChatStore.getState().useAnyTxtSearch,
        agentMode: useChatStore.getState().agentMode,
        retrievalMode: useChatStore.getState().retrievalMode,
        skills: useChatStore.getState().selectedSkills,
        contextFiles: useChatStore.getState().selectedContextFiles,
        skillMode: useChatStore.getState().selectedSkills.length > 0 ? "explicit" : "auto",
        approvedShellCommands: [command.trim()],
        shellCommand: command.trim(),
        suppressUserMessage: true,
        historyOverride: resumeHistory,
      })
    } finally {
      setApprovingShellMessageId(null)
    }
  }, [approvingShellMessageId, handleSend, setStreaming])

  const handleSubmitUserInput = useCallback((request: ChatUserInputRequest, answers: Record<string, unknown>) => {
    if (activeStreaming) return false
    const answerLines = request.fields.map((field) => {
      const value = answers[field.id]
      const rendered = Array.isArray(value) ? value.join(", ") : String(value ?? "")
      return `- ${field.label} (${field.id}): ${rendered || "(empty)"}`
    })
    const resumeMessage = [
      `User provided answers for "${request.title}".`,
      "",
      ...answerLines,
      "",
      "Continue the previous task using these answers. Do not ask the same questions again unless required information is still missing.",
    ].join("\n")
    handleSend(resumeMessage, [], {
      useWebSearch: useChatStore.getState().useWebSearch,
      useAnyTxtSearch: useChatStore.getState().useAnyTxtSearch,
      agentMode: useChatStore.getState().agentMode,
      retrievalMode: useChatStore.getState().retrievalMode,
      skills: useChatStore.getState().selectedSkills,
      contextFiles: useChatStore.getState().selectedContextFiles,
      skillMode: useChatStore.getState().selectedSkills.length > 0 ? "explicit" : "auto",
    })
    return true
  }, [handleSend, activeStreaming])

  const handleWriteToWiki = useCallback(async () => {
    if (!project) return
    const pp = normalizePath(project.path)
    try {
      await executeIngestWrites(pp, llmConfig, undefined, undefined)
      await refreshProjectFileTree(pp, { bumpDataVersion: true })
    } catch (err) {
      console.error("Failed to write to wiki:", err)
    }
  }, [project, llmConfig])

  const hasAssistantMessages = activeMessages.some((m) => m.role === "assistant")
  useEffect(() => {
    const stop = [
      registerDesktopAction("chat.send", "(text: string): { started: true }", text => {
        if (typeof text !== "string" || !text.trim()) throw new Error("text must be a nonempty string")
        if (useChatStore.getState().isStreaming) throw new Error("A chat turn is already active")
        // Turns can outlive a CLI request. Progress/results remain in chat state.
        void handleSend(text).catch(error => console.error("CLI chat turn failed:", error))
        return { started: true }
      }),
      registerDesktopAction("chat.stop", "(): void", handleStop),
      registerDesktopAction("chat.newConversation", "(): void", handleNewConversation),
      registerDesktopAction("chat.regenerate", "(): { started: true }", () => {
        void handleRegenerate().catch(error => console.error("CLI regeneration failed:", error))
        return { started: true }
      }),
      registerDesktopAction("chat.writeToWiki", "(): Promise<void>", handleWriteToWiki),
    ]
    return () => stop.forEach(unregister => unregister())
  }, [handleSend, handleStop, handleNewConversation, handleRegenerate, handleWriteToWiki])
  const showWriteButton = mode === "ingest" && !activeStreaming && hasAssistantMessages

  const runtimeLabel = harnessStatus.state === "checking"
    ? t("chat.agent.checking")
    : harnessStatus.state === "ready"
      ? t("chat.agent.ready", { provider: harnessStatus.provider, model: harnessStatus.model })
      : t("chat.agent.offline")

  return (
    <div className="relative flex h-full flex-row overflow-hidden">
      <ConversationSidebar
        onNewConversation={handleNewConversation}
        onSelectConversation={handleSelectConversation}
      />

      <div className="flex flex-1 flex-col overflow-hidden">
        {mode === "chat" && (
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b bg-muted/20 px-3 py-2 text-xs">
            <label className="flex items-center gap-2">
              Agent
              <select aria-label="Agent runtime" value={agentRuntime} disabled={activeStreaming}
                className="max-w-48 rounded border bg-background px-2 py-1"
                onChange={(event) => setAgentRuntime(event.target.value as "native" | "harness")}>
                <option value="native">Athenaeum Agent</option>
                <option value="harness">Harness (advanced)</option>
              </select>
            </label>
            {agentRuntime === "native" && <span className="text-muted-foreground">Uses Athenaeum Agent’s model configuration or your OpenAI API settings.</span>}
            <span className="flex min-w-0 items-center gap-1.5" title={project?.path ?? t("chat.agent.noWorkspace")}>
              <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted-foreground" aria-hidden />
              <span className="truncate font-medium">{project?.path ?? t("chat.agent.noWorkspace")}</span>
            </span>
            <span
              className={`flex min-w-0 items-center gap-1.5 ${harnessStatus.state === "offline" ? "text-destructive" : "text-muted-foreground"}`}
              role="status"
              aria-live="polite"
              title={harnessStatus.state === "offline" ? harnessStatus.message : runtimeLabel}
            >
              {harnessStatus.state === "checking"
                ? <LoaderCircle className="h-3.5 w-3.5 shrink-0 animate-spin" aria-hidden />
                : harnessStatus.state === "ready"
                  ? <CircleCheck className="h-3.5 w-3.5 shrink-0 text-emerald-600" aria-hidden />
                  : <CircleAlert className="h-3.5 w-3.5 shrink-0" aria-hidden />}
              <span className="truncate">{harnessStatus.state === "offline"
                ? `${runtimeLabel}: ${harnessStatus.message}`
                : runtimeLabel}</span>
            </span>
            <span className="hidden min-w-0 flex-1 truncate text-muted-foreground lg:inline" title={t("chat.agent.proposalReminder")}>
              {t("chat.agent.proposalReminder")}
            </span>
            <Button
              variant="outline"
              size="sm"
              className="ml-auto h-7 shrink-0 gap-1.5 px-2 text-xs"
              onClick={() => setActiveView("review")}
            >
              <ClipboardCheck className="h-3.5 w-3.5" aria-hidden />
              {t("chat.agent.reviewProposals")}
            </Button>
          </div>
        )}
        {!activeConversationId ? (
          <div className="flex flex-1 items-center justify-center text-muted-foreground">
            <div className="max-w-md space-y-3 px-6 text-center">
              <MessageSquare className="mx-auto mb-3 h-8 w-8 opacity-30" />
              {mode === "chat" && harnessStatus.state === "offline" ? <>
                <h2 className="font-medium text-foreground">Chat is not ready</h2>
                <p className="text-sm">In-app chat needs its own model and agent runtime. You can keep using your connected coding agent with wiki memory.</p>
                <div className="flex flex-wrap justify-center gap-2">
                  <Button variant="outline" onClick={() => openConnectors("agents")}>Coding agents</Button>
                  <Button variant="ghost" onClick={() => openConnectors("model")}>AI model settings</Button>
                </div>
              </> : <>
              <p className="text-sm">{t("chat.startNewConversation")}</p>
              <p className="mt-1 text-xs opacity-60">{t("chat.clickNewChatToBegin")}</p>
              <Button variant="outline" onClick={handleNewConversation}>New chat</Button>
              </>}
            </div>
          </div>
          ) : (
            <>
              <div
                ref={scrollContainerRef}
                className="flex-1 overflow-y-auto px-3 py-2"
              >
                <div className="flex flex-col gap-3">
                  {activeMessages.map((msg, idx) => {
                    // Check if this is the last assistant message
                    const isLastAssistant = msg.role === "assistant" &&
                      !activeMessages.slice(idx + 1).some((m) => m.role === "assistant")
                    return (
                      <ChatMessage
                        key={`${msg.conversationId}:${msg.id}:${msg.timestamp}:${idx}`}
                        message={msg}
                        isLastAssistant={isLastAssistant && !activeStreaming}
                        onRegenerate={isLastAssistant ? handleRegenerate : undefined}
                        onOpenReferencePreview={handleOpenReferencePreview}
                        onOpenContextDetails={handleOpenContextDetails}
                        onApproveShellCommand={
                          mode !== "chat" && isLastAssistant && approvingShellMessageId !== msg.id
                            ? handleApproveShellCommand
                            : undefined
                        }
                        onSubmitUserInput={isLastAssistant ? handleSubmitUserInput : undefined}
                      />
                    )
                  })}
                  {activeStreaming && <StreamingMessage content={streamingContent} agentEvents={activeAgentEvents} />}
                  <div ref={bottomRef} />
                </div>
              </div>

            {showWriteButton && (
              <div className="border-t px-3 py-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleWriteToWiki}
                  className="w-full gap-2"
                >
                  <BookOpen className="h-4 w-4" />
                  {t("chat.writeToWiki")}
                </Button>
              </div>
            )}
          </>
        )}

        <ChatInput
          onSend={handleSend}
          onStop={handleStop}
          isStreaming={activeStreaming}
          useWebSearch={useWebSearch}
          useAnyTxtSearch={useAnyTxtSearch}
          agentMode={agentMode}
          retrievalMode={retrievalMode}
          availableSkills={availableSkills}
          selectedSkills={selectedSkills}
          availableContextFiles={availableContextFiles}
          selectedContextFiles={selectedContextFiles}
          onUseWebSearchChange={setUseWebSearch}
          onUseAnyTxtSearchChange={setUseAnyTxtSearch}
          onAgentModeChange={setAgentMode}
          onRetrievalModeChange={setRetrievalMode}
          onSelectedSkillsChange={setSelectedSkills}
          onSelectedContextFilesChange={setSelectedContextFiles}
          anyTxtAvailable={anyTxtAvailable}
          imageInputAvailable={imageInputAvailable}
          harnessManaged={mode === "chat"}
          placeholder={
            mode === "ingest"
              ? t("chat.ingestPlaceholder")
              : t("chat.typeAMessage")
          }
        />
      </div>

      {referencePreview && (
        <ChatReferencePreviewPanel
          preview={referencePreview}
          width={referencePreviewWidth}
          onResize={setReferencePreviewWidth}
          onClose={() => setReferencePreview(null)}
        />
      )}
      {contextDetailReferences && project && (
        <ContextDetailsPanel
          references={contextDetailReferences}
          projectPath={project.path}
          width={referencePreviewWidth}
          onResize={setReferencePreviewWidth}
          onClose={() => setContextDetailReferences(null)}
        />
      )}
      {approval && (
        <section role="region" aria-label={approval.title} className="absolute bottom-20 left-4 right-4 z-30 space-y-3 rounded-md border bg-background p-4">
          <h3 className="text-sm font-medium">{approval.title}</h3>
          <pre className="max-h-40 overflow-auto whitespace-pre-wrap break-words text-xs">{approval.detail}</pre>
          {approvalError && <p role="alert" className="text-xs text-destructive">{approvalError}</p>}
          <div className="flex gap-2">
            {(["accept", "decline"] as const).map((decision) => (
              <Button key={decision} variant={decision === "accept" ? "default" : "outline"} disabled={approvalBusy}
                onClick={() => {
                  setApprovalBusy(true)
                  void invoke("agent_respond_approval", { approvalId: approval.id, decision })
                    .then(() => setApproval(null)).catch((cause) => setApprovalError(String(cause)))
                    .finally(() => setApprovalBusy(false))
                }}>{decision === "accept" ? "Allow once" : "Deny"}</Button>
            ))}
          </div>
        </section>
      )}
      {generatedOutputPreviews.length > 0 && (
        <GeneratedOutputsPanel
          outputs={generatedOutputPreviews}
          onOpen={openGeneratedOutputModal}
          onOpenDirectory={project ? openGeneratedOutputDirectory : undefined}
          onClose={closeGeneratedOutputsPanel}
        />
      )}
      {generatedOutputPreview && (
        <GeneratedOutputPreviewDialog
          preview={generatedOutputPreview}
          onClose={() => setGeneratedOutputPreview(null)}
        />
      )}
    </div>
  )
}

function GeneratedOutputsPanel({
  outputs,
  onOpen,
  onOpenDirectory,
  onClose,
}: {
  outputs: ChatReferencePreview[]
  onOpen: (preview: ChatReferencePreview) => void
  onOpenDirectory?: () => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  return (
    <aside className="flex h-full w-[280px] shrink-0 flex-col border-l bg-background">
      <div className="flex min-h-10 items-center gap-2 border-b px-3 py-2">
        <FolderOpen className="h-4 w-4 shrink-0 text-primary" />
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-medium">{t("chat.generatedOutputs")}</div>
          <div className="mt-0.5 text-[10px] text-muted-foreground">
            {t("chat.generatedOutputCount", { count: outputs.length })}
          </div>
        </div>
        {onOpenDirectory && (
          <button
            type="button"
            onClick={onOpenDirectory}
            className="shrink-0 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
            title={t("chat.openGeneratedOutputFolder", { defaultValue: "Open output folder" })}
            aria-label={t("chat.openGeneratedOutputFolder", { defaultValue: "Open output folder" })}
          >
            <FolderOpen className="h-3.5 w-3.5" />
          </button>
        )}
        <button
          type="button"
          onClick={onClose}
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
          title={t("chat.closeGeneratedOutputs")}
          aria-label={t("chat.closeGeneratedOutputs")}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto p-2">
        <div className="space-y-1">
          {outputs.map((output) => {
            const title = output.title || getFileName(output.path)
            const isImageOutput = isGeneratedOutputImage(output.path)
            const imageSrc = isImageOutput ? convertFileSrc(output.path) : null
            return (
              <button
                key={output.path}
                type="button"
                onClick={() => onOpen(output)}
                className="group flex w-full items-start gap-2 rounded-md border border-border/60 bg-muted/20 px-2 py-2 text-left transition-colors hover:border-primary/30 hover:bg-primary/5"
                title={output.path}
              >
                {imageSrc ? (
                  <span className="h-10 w-12 shrink-0 overflow-hidden rounded border border-primary/20 bg-background/80">
                    <img
                      src={imageSrc}
                      alt={title}
                      loading="lazy"
                      className="h-full w-full object-cover"
                      onError={(event) => {
                        event.currentTarget.style.opacity = "0"
                      }}
                    />
                  </span>
                ) : (
                  <FileText className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground group-hover:text-primary" />
                )}
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-xs font-medium text-foreground">{title}</span>
                  <span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{output.path}</span>
                </span>
                <Maximize2 className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground group-hover:text-primary" />
              </button>
            )
          })}
        </div>
      </div>
    </aside>
  )
}

function GeneratedOutputPreviewDialog({
  preview,
  onClose,
}: {
  preview: ChatReferencePreview
  onClose: () => void
}) {
  const { t } = useTranslation()
  const displayTitle = preview.title || getFileName(preview.path)
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose()
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [onClose])
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 p-6">
      <div className="flex h-[86vh] w-[80vw] min-w-0 max-w-[1600px] flex-col overflow-hidden rounded-xl border bg-background shadow-2xl">
        <div className="flex min-h-12 items-center gap-3 border-b px-4 py-2">
          <Maximize2 className="h-4 w-4 shrink-0 text-primary" />
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium" title={displayTitle}>{displayTitle}</div>
            <div className="mt-0.5 truncate text-[11px] text-muted-foreground" title={preview.path}>{preview.path}</div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="shrink-0 rounded p-1.5 text-muted-foreground hover:bg-accent hover:text-foreground"
            title={t("chat.closeGeneratedOutputPreview")}
            aria-label={t("chat.closeGeneratedOutputPreview")}
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-hidden">
          <ChatReferencePreviewContent preview={preview} />
        </div>
      </div>
    </div>
  )
}

function ChatReferencePreviewPanel({
  preview,
  width,
  onResize,
  onClose,
}: {
  preview: ChatReferencePreview
  width: number
  onResize: (width: number) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const displayTitle = preview.title || getFileName(preview.path)
  const dragStartRef = useRef<{ x: number; width: number } | null>(null)

  const startResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    dragStartRef.current = { x: event.clientX, width }
    event.currentTarget.setPointerCapture(event.pointerId)
  }, [width])

  const handleResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragStartRef.current) return
    const delta = dragStartRef.current.x - event.clientX
    onResize(clampReferencePreviewWidth(dragStartRef.current.width + delta))
  }, [onResize])

  const stopResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    dragStartRef.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId)
    }
  }, [])

  return (
    <aside
      className="relative flex h-full min-w-[320px] max-w-[56%] shrink-0 flex-col border-l bg-background"
      style={{ width }}
    >
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t("chat.resizeReferencePreview")}
        tabIndex={0}
        onPointerDown={startResize}
        onPointerMove={handleResize}
        onPointerUp={stopResize}
        onPointerCancel={stopResize}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft") {
            event.preventDefault()
            onResize(clampReferencePreviewWidth(width + 32))
          } else if (event.key === "ArrowRight") {
            event.preventDefault()
            onResize(clampReferencePreviewWidth(width - 32))
          }
        }}
        className="absolute -left-1 top-0 z-10 h-full w-2 cursor-col-resize outline-none transition-colors hover:bg-primary/15 focus-visible:bg-primary/20"
      />
      <div className="flex min-h-10 items-center gap-2 border-b px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="truncate text-xs font-medium" title={displayTitle}>
            {displayTitle}
          </div>
          <div className="mt-0.5 truncate text-[10px] text-muted-foreground" title={preview.path}>
            {preview.source ?? t("chat.referencePreview")} · {preview.path}
          </div>
        </div>
        <button
          type="button"
          onClick={onClose}
          className="shrink-0 rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
          title={t("chat.closeReferencePreview")}
          aria-label={t("chat.closeReferencePreview")}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <ChatReferencePreviewContent preview={preview} />
      </div>
    </aside>
  )
}

function ContextDetailsPanel({
  references,
  projectPath,
  width,
  onResize,
  onClose,
}: {
  references: MessageReference[]
  projectPath: string
  width: number
  onResize: (width: number) => void
  onClose: () => void
}) {
  const { t } = useTranslation()
  const [items, setItems] = useState<ContextDetailItem[]>([])
  const [loading, setLoading] = useState(true)
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})
  const dragStartRef = useRef<{ x: number; width: number } | null>(null)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    Promise.all(references.map(async (reference): Promise<ContextDetailItem> => {
      let content = reference.snippet?.trim() ?? ""
      // The persisted snippet is the exact retrieved fragment. Prefer it over
      // re-reading a file that may have changed since the answer was produced.
      if (reference.kind !== "external" && !content) {
        const normalizedProject = normalizePath(projectPath)
        const directPath = isAbsolutePath(reference.path)
          ? normalizePath(reference.path)
          : projectAbsolutePath(normalizedProject, reference.path)
        const stem = getFileName(reference.path.replace(/^wiki\//, "").replace(/\.md$/i, ""))
        const candidates = reference.kind === "workspace"
          ? [directPath]
          : [
              directPath,
              `${normalizedProject}/wiki/entities/${stem}.md`,
              `${normalizedProject}/wiki/concepts/${stem}.md`,
              `${normalizedProject}/wiki/sources/${stem}.md`,
              `${normalizedProject}/wiki/queries/${stem}.md`,
              `${normalizedProject}/wiki/synthesis/${stem}.md`,
              `${normalizedProject}/wiki/comparisons/${stem}.md`,
              `${normalizedProject}/wiki/${stem}.md`,
            ]
        for (const candidate of candidates) {
          try {
            content = await readFile(candidate)
            break
          } catch {
            // Try resolver-compatible fallback paths.
          }
        }
      }
      const category = contextDetailCategory(reference)
      if (category === "graph" && reference.graphRelations?.length) {
        content = [
          content,
          content ? "" : undefined,
          ...reference.graphRelations.map((relation) => `→ ${relation}`),
        ].filter((line): line is string => typeof line === "string").join("\n")
      }
      return {
        ...reference,
        content,
        charCount: content.length,
        category,
      }
    })).then((loaded) => {
      if (cancelled) return
      const graphItems: ContextDetailItem[] = loaded.flatMap((item) => {
        if (item.category === "graph" || !item.graphRelations?.length) return []
        const content = item.graphRelations.join("\n")
        return [{
          title: item.title,
          path: item.path,
          kind: "wiki",
          source: "Graph",
          graphRelations: item.graphRelations,
          content,
          charCount: content.length,
          category: "graph" as const,
        }]
      })
      setItems([...loaded, ...graphItems])
    }).finally(() => {
      if (!cancelled) setLoading(false)
    })
    return () => { cancelled = true }
  }, [projectPath, references])

  const groups = useMemo(() => {
    const result = new Map<ContextDetailCategory, ContextDetailItem[]>()
    for (const item of items) {
      result.set(item.category, [...(result.get(item.category) ?? []), item])
    }
    const labels: Record<ContextDetailCategory, string> = {
      wiki: t("chat.contextCategoryWiki"),
      graph: t("chat.contextCategoryGraph"),
      web: t("chat.contextCategoryWeb"),
      anytxt: t("chat.contextCategoryAnyTxt"),
      workspace: t("chat.contextCategoryWorkspace"),
      external: t("chat.contextCategoryExternal"),
    }
    const order: ContextDetailCategory[] = ["wiki", "graph", "web", "anytxt", "workspace", "external"]
    return order.flatMap((category) => {
      const group = result.get(category)
      return group ? [[category, labels[category], group] as const] : []
    })
  }, [items, t])
  const totalChars = items.reduce((sum, item) => sum + item.charCount, 0)
  const estimatedTokens = Math.ceil(totalChars / 4)

  const startResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    event.preventDefault()
    dragStartRef.current = { x: event.clientX, width }
    event.currentTarget.setPointerCapture(event.pointerId)
  }, [width])
  const resize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!dragStartRef.current) return
    onResize(clampReferencePreviewWidth(dragStartRef.current.width + dragStartRef.current.x - event.clientX))
  }, [onResize])
  const stopResize = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    dragStartRef.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }, [])

  return (
    <aside className="relative flex h-full min-w-[320px] max-w-[56%] shrink-0 flex-col border-l bg-background" style={{ width }}>
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t("chat.resizeReferencePreview")}
        className="absolute -left-1 top-0 z-10 h-full w-2 cursor-col-resize hover:bg-primary/15"
        onPointerDown={startResize}
        onPointerMove={resize}
        onPointerUp={stopResize}
        onPointerCancel={stopResize}
      />
      <div className="flex min-h-10 items-center gap-2 border-b px-3 py-2">
        <ListTree className="h-4 w-4 text-primary" />
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium">{t("chat.contextDetails")}</div>
          <div className="text-[10px] text-muted-foreground">{t("chat.contextDetailsScope")}</div>
        </div>
        <button type="button" onClick={onClose} className="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground" aria-label={t("chat.closeReferencePreview")}>
          <X className="h-3.5 w-3.5" />
        </button>
      </div>
      <div className="grid grid-cols-3 border-b bg-muted/20 text-center text-[11px]">
        <div className="border-r px-2 py-2"><strong className="block text-sm text-foreground">{items.length}</strong>{t("chat.contextItems")}</div>
        <div className="border-r px-2 py-2"><strong className="block text-sm text-foreground">{totalChars.toLocaleString()}</strong>{t("chat.contextCharacters")}</div>
        <div className="px-2 py-2"><strong className="block text-sm text-foreground">~{estimatedTokens.toLocaleString()}</strong>{t("chat.contextTokens")}</div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {loading ? (
          <div className="p-4 text-xs text-muted-foreground">{t("common.loading")}</div>
        ) : groups.map(([category, label, group]) => (
          <section key={category} className="border-b">
            <div className="sticky top-0 z-[1] flex items-center justify-between border-b bg-muted/70 px-3 py-2 text-[11px] font-semibold backdrop-blur">
              <span>{label}</span>
              <span className="rounded bg-background px-1.5 py-0.5 text-[10px] text-muted-foreground">{group.length}</span>
            </div>
            {category === "graph" && (
              <div className="px-3 pt-2">
                <ReferenceKnowledgeGraph references={group} />
              </div>
            )}
            {group.map((item, index) => {
              const key = `${item.kind ?? "wiki"}:${item.path}:${index}`
              const isExpanded = Boolean(expanded[key])
              return (
                <div key={key} className="border-t border-border/50 px-3 py-2">
                  <button type="button" className="flex w-full items-start gap-2 text-left" onClick={() => setExpanded((value) => ({ ...value, [key]: !isExpanded }))}>
                    {isExpanded ? <ChevronDown className="mt-0.5 h-3.5 w-3.5" /> : <ChevronRight className="mt-0.5 h-3.5 w-3.5" />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-xs font-medium">{item.title}</span>
                      <span className="block truncate text-[10px] text-muted-foreground" title={item.url ?? item.path}>{item.url ?? item.path}</span>
                    </span>
                    <span className="shrink-0 text-[10px] text-muted-foreground">
                      {item.category === "graph"
                        ? t("chat.contextRelations", { count: item.graphRelations?.length ?? 0 })
                        : item.charCount > 0
                          ? t("chat.contextCharacterCount", { count: item.charCount.toLocaleString() })
                          : t("chat.contextNotRecorded")}
                    </span>
                  </button>
                  {isExpanded && (
                    <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-words border-l-2 border-primary/30 pl-3 text-[11px] leading-5 text-foreground/80">{item.content || t("chat.contextContentUnavailable")}</pre>
                  )}
                </div>
              )
            })}
          </section>
        ))}
      </div>
    </aside>
  )
}

function ChatReferencePreviewContent({ preview }: { preview: ChatReferencePreview }) {
  if (preview.external) return <ExternalReferencePreview preview={preview} />
  if (getFileCategory(preview.path) === "markdown") {
    return <ChatMarkdownReferencePreview preview={preview} />
  }
  return (
    <FilePreview
      key={preview.path}
      filePath={preview.path}
      textContent={preview.content}
    />
  )
}

function clampReferencePreviewWidth(width: number): number {
  return Math.min(760, Math.max(320, Math.round(width)))
}

function ChatMarkdownReferencePreview({ preview }: { preview: ChatReferencePreview }) {
  const { frontmatter, body } = parseFrontmatter(preview.content)
  return (
    <div className="h-full overflow-auto px-6 py-6">
      {frontmatter && <FrontmatterPanel data={frontmatter} />}
      <WikiReader body={body} filePath={preview.path} />
    </div>
  )
}

function ExternalReferencePreview({ preview }: { preview: ChatReferencePreview }) {
  const { t } = useTranslation()
  return (
    <div className="flex h-full flex-col overflow-auto p-5">
      <div className="mb-4 space-y-2">
        <div className="flex items-center gap-2">
          {preview.source && (
            <span className="rounded border border-border/60 bg-muted px-1.5 py-0.5 text-[10px] font-medium uppercase text-muted-foreground">
              {preview.source}
            </span>
          )}
          <h3 className="truncate text-sm font-medium" title={preview.title}>{preview.title}</h3>
        </div>
        <div className="break-all rounded-md border border-border/60 bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          {preview.path.replace(/^[a-z]+-preview:\/\//, "")}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-border/60 bg-muted/20 p-4">
        <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-6">
          {preview.snippet?.trim() || t("chat.noReferencePreviewFragment")}
        </pre>
      </div>
    </div>
  )
}

function isAbortLikeError(err: unknown): boolean {
  if (err instanceof DOMException && err.name === "AbortError") return true
  if (!(err instanceof Error)) return false
  return err.name === "AbortError" || /abort|cancel/i.test(err.message)
}
