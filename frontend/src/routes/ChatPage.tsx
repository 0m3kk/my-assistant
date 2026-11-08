import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react"
import type { FormEvent } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useNavigate, useParams, useSearchParams } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { isToolUIPart } from "ai"
import { useChat, type Chat } from "@ai-sdk/react"
import type { UIMessage } from "ai"
import { ArrowUpIcon, CircleStopIcon, FolderIcon, FolderOpenIcon, FolderPlusIcon, MessageSquareIcon, PaperclipIcon, SparklesIcon, XIcon } from "lucide-react"
import { toast } from "sonner"

import { conversationGroupsApi, conversationsApi, feedbackApi, filesApi, workspaceApi } from "@/lib/api/endpoints"
import { REASONING_EFFORT_LEVELS, type ConversationSummary, type FileDto, type ReasoningEffort } from "@/lib/types"
import { cn } from "@/lib/utils"
import { getChat, hasChat, runtimeState } from "@/lib/ai/chat-manager"
import {
  clearPendingGroup,
  getPendingGroup,
  setActiveFolder,
  setPendingGroup,
  useActiveFolder,
} from "@/lib/chat/folder-selection"
import { Button } from "@/components/ui/button"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupTextarea } from "@/components/ui/input-group"
import { ConversationSidebar } from "@/components/chat/ConversationSidebar"
import { ContextMeter } from "@/components/chat/ContextMeter"
import { AgentBadge } from "@/components/chat/AgentBadge"
import { MessageRenderer } from "@/components/chat/MessageRenderer"
import { ModelSelector } from "@/components/chat/ModelSelector"
import { WorkspacePanel } from "@/components/chat/WorkspacePanel"
import { usePanelRef } from "react-resizable-panels"
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  MessageScrollerProvider,
  MessageScroller,
  MessageScrollerViewport,
  MessageScrollerContent,
  MessageScrollerItem,
} from "@/components/ui/message-scroller"
import { Spinner } from "@/components/shared/Spinner"
import { GroupNameDialog } from "@/components/shared/GroupNameDialog"

const SIDEBAR_LAYOUT_KEY = "assistant_sidebar_layout"
const SIDEBAR_COLLAPSED_KEY = "assistant_sidebar_collapsed"
const EFFORT_OVERRIDE_KEY = "assistant_effort_override"

function defaultSidebarLayout(): Record<string, number> {
  try {
    const saved = localStorage.getItem(SIDEBAR_LAYOUT_KEY)
    if (saved) return JSON.parse(saved) as Record<string, number>
  } catch {
    // ignore malformed state
  }
  return { sidebar: 15, main: 85 }
}

/** Restored across reloads; invalid or absent values fall back to "Default". */
function loadEffortOverride(): string | null {
  try {
    const saved = localStorage.getItem(EFFORT_OVERRIDE_KEY)
    if (saved && REASONING_EFFORT_LEVELS.includes(saved as ReasoningEffort)) return saved
  } catch {
    // storage unavailable
  }
  return null
}

function loadSidebarCollapsed(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_COLLAPSED_KEY) === "1"
  } catch {
    return false
  }
}

export function ChatPage() {
  const { conversationId } = useParams()
  // A draft chat can be started inside a folder: `/chat?folder=<id>`.
  const [searchParams] = useSearchParams()
  const draftFolderId = searchParams.get("folder")
  const queryClient = useQueryClient()
  const [modelId, setModelId] = useState<string | null>(null)
  const [effort, setEffort] = useState<string | null>(loadEffortOverride)
  const sidebarPanelRef = usePanelRef()
  const [sidebarCollapsed, setSidebarCollapsed] = useState(loadSidebarCollapsed)

  useEffect(() => {
    try {
      localStorage.setItem(SIDEBAR_COLLAPSED_KEY, sidebarCollapsed ? "1" : "0")
    } catch {
      // storage unavailable — state stays session-only
    }
  }, [sidebarCollapsed])

  // Apply the persisted state before the first paint.
  useLayoutEffect(() => {
    if (loadSidebarCollapsed()) sidebarPanelRef.current?.collapse()
  }, [sidebarPanelRef])

  const toggleSidebar = () => {
    const panel = sidebarPanelRef.current
    if (!panel) return
    if (panel.isCollapsed()) {
      panel.expand()
      setSidebarCollapsed(false)
    } else {
      panel.collapse()
      setSidebarCollapsed(true)
    }
  }

  const onConversationsChanged = useCallback(
    () => void queryClient.invalidateQueries({ queryKey: ["conversations"] }),
    [queryClient],
  )

  // The transport reads the selected model/effort at send time; an explicit
  // effort override is remembered across reloads and model switches.
  useEffect(() => {
    runtimeState.modelId = modelId
  }, [modelId])
  useEffect(() => {
    runtimeState.reasoningEffort = effort
    try {
      if (effort) localStorage.setItem(EFFORT_OVERRIDE_KEY, effort)
      else localStorage.removeItem(EFFORT_OVERRIDE_KEY)
    } catch {
      // storage unavailable — override stays session-only
    }
  }, [effort])

  return (
    <ResizablePanelGroup
      orientation="horizontal"
      className="h-dvh overflow-hidden"
      defaultLayout={defaultSidebarLayout()}
      onLayoutChanged={(layout, meta) => {
        if (meta.isUserInteraction) {
          // Don't persist a collapsed/dragged-past-minimum layout, so a later
          // expand restores the previous readable width.
          const sidebar = layout.sidebar
          if (typeof sidebar === "number" && sidebar > 0 && sidebar < 10) return
          localStorage.setItem(SIDEBAR_LAYOUT_KEY, JSON.stringify(layout))
        }
      }}
    >
      <ResizablePanel
        id="sidebar"
        panelRef={sidebarPanelRef}
        collapsible
        collapsedSize="4rem"
        defaultSize="15%"
        minSize="14%"
        maxSize="45%"
        onResize={() => setSidebarCollapsed(sidebarPanelRef.current?.isCollapsed() ?? false)}
      >
        <ConversationSidebar collapsed={sidebarCollapsed} onToggleCollapsed={toggleSidebar} />
      </ResizablePanel>
      <ResizableHandle withHandle />
      <ResizablePanel id="main">
        <ChatInner
          key={conversationId ?? `root:${draftFolderId ?? ""}`}
          conversationId={conversationId}
          draftFolderId={draftFolderId}
          sidebarCollapsed={sidebarCollapsed}
          modelId={modelId}
          effort={effort}
          onModelChange={setModelId}
          onEffortChange={setEffort}
          onConversationsChanged={onConversationsChanged}
        />
      </ResizablePanel>
    </ResizablePanelGroup>
  )
}

function ChatInner({
  conversationId,
  draftFolderId,
  sidebarCollapsed,
  modelId,
  effort,
  onModelChange,
  onEffortChange,
  onConversationsChanged,
}: {
  conversationId?: string
  draftFolderId: string | null
  sidebarCollapsed: boolean
  modelId: string | null
  effort: string | null
  onModelChange: (id: string) => void
  onEffortChange: (effort: string | null) => void
  onConversationsChanged: () => void
}) {
  // Draft conversations pre-allocate a client-side uuid so the Chat instance
  // (and its in-flight stream) is stable from the very first message.
  const [draftId] = useState(() => crypto.randomUUID())
  const activeId = conversationId ?? draftId

  // `?folder=` is a hint for a draft opened directly; the shared selection
  // drives the header badge and the first send afterwards.
  useEffect(() => {
    if (!conversationId && draftFolderId) setActiveFolder(draftFolderId)
  }, [conversationId, draftFolderId])

  const initial = useQuery({
    queryKey: ["messages", activeId],
    queryFn: () => conversationsApi.messages(activeId),
    enabled: !!conversationId && !hasChat(activeId),
    staleTime: 30_000,
  })

  const ready = !conversationId || hasChat(activeId) || initial.isSuccess
  const chat = useMemo<Chat<UIMessage> | null>(
    () => (ready ? getChat(activeId, initial.data) : null),
    [ready, activeId, initial.data],
  )

  if (!ready || !chat) {
    return (
      <main className="flex h-full w-full min-w-0 items-center justify-center">
        <Spinner />
      </main>
    )
  }

  return (
    <ChatReady
      chat={chat}
      activeId={activeId}
      isNew={!conversationId}
      sidebarCollapsed={sidebarCollapsed}
      modelId={modelId}
      effort={effort}
      onModelChange={onModelChange}
      onEffortChange={onEffortChange}
      onConversationsChanged={onConversationsChanged}
    />
  )
}

function ChatReady({
  chat,
  activeId,
  isNew,
  sidebarCollapsed,
  modelId,
  effort,
  onModelChange,
  onEffortChange,
  onConversationsChanged,
}: {
  chat: Chat<UIMessage>
  activeId: string
  isNew: boolean
  sidebarCollapsed: boolean
  modelId: string | null
  effort: string | null
  onModelChange: (id: string) => void
  onEffortChange: (effort: string | null) => void
  onConversationsChanged: () => void
}) {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const formRef = useRef<HTMLFormElement>(null)
  const [draft, setDraft] = useState("")
  // Folder selected in the sidebar; drafts use it as their destination.
  const selectedFolderId = useActiveFolder()
  const [folderDialogOpen, setFolderDialogOpen] = useState(false)
  // ----- Attachments -----
  const [pendingFiles, setPendingFiles] = useState<FileDto[]>([])
  const [uploadingCount, setUploadingCount] = useState(0)
  const fileInputRef = useRef<HTMLInputElement>(null)
  // ----- Workspace exploration panel -----
  const [workspaceOpen, setWorkspaceOpen] = useState(false)
  // Bottom-pinning is managed here, NOT via the primitive's autoScroll: its
  // "following-bottom" mode yanks the viewport to the bottom on ANY content
  // resize (e.g. expanding a reasoning block — even from a state where the
  // content previously fit and didn't scroll at all). Instead we track the
  // user's position on scroll and pin explicitly while streaming.
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const atBottomRef = useRef(true)

  const { messages, sendMessage, status, stop, addToolApprovalResponse, setMessages, resumeStream } =
    useChat({ chat })

  const busy = status === "submitted" || status === "streaming"

  // Stay in a "working" state for the whole turn except while text/reasoning
  // is actively streaming. Tool runs (and the pause after a tool fails before
  // the next model step) still count as working, so the UI never looks idle
  // mid-run.
  const lastMessage = messages[messages.length - 1]
  const lastPart =
    lastMessage?.role === "assistant" ? lastMessage.parts[lastMessage.parts.length - 1] : undefined
  const streamingOutput =
    lastPart != null &&
    (lastPart.type === "text" || lastPart.type === "reasoning") &&
    lastPart.text.trim().length > 0
  const showWorking = busy && !streamingOutput

  // Existing feedback for this conversation (tool + agent runs).
  const feedbackQuery = useQuery({
    queryKey: ["feedback", activeId],
    queryFn: () => feedbackApi.list(activeId).then((r) => r.feedback),
    enabled: !isNew,
    staleTime: 10_000,
  })

  // Approval ledger (approval cards + history badges on tool runs).
  const approvalsQuery = useQuery({
    queryKey: ["approvals", activeId],
    queryFn: () => conversationsApi.approvals(activeId),
    enabled: !isNew,
    staleTime: 5_000,
  })

  // Folder badge: existing conversations read it from the list query, drafts
  // keep their chosen folder locally until the first send.
  const groupsQuery = useQuery({
    queryKey: ["conversation-groups"],
    queryFn: () => conversationGroupsApi.list(),
  })
  const conversationsQuery = useQuery({
    queryKey: ["conversations"],
    queryFn: () => conversationsApi.list(),
    // Also needed when collapsed: the header folder menu switches conversations.
    enabled: !isNew || sidebarCollapsed,
  })

  // A run started elsewhere (another tab, or before a reload) keeps going on
  // the server. Poll the stored transcript while it runs so reopening the
  // conversation eventually shows the result without a manual refresh.
  // This tab's own turn already arrives over the SSE stream — polling it too
  // would refetch the whole transcript every 1.5s for nothing, so the poll
  // only covers runs this client is not streaming.
  const conversationRunning =
    !isNew && (conversationsQuery.data?.find((c) => c.id === activeId)?.status === "running")
  const pollStoredMessages = conversationRunning && !busy
  const storedMessages = useQuery({
    queryKey: ["messages", activeId],
    queryFn: () => conversationsApi.messages(activeId),
    enabled: !isNew,
    refetchInterval: pollStoredMessages ? 1_500 : false,
    staleTime: 0,
  })

  const lastSyncedRef = useRef("")
  useEffect(() => {
    if (isNew || busy) return
    const stored = storedMessages.data
    if (!stored) return
    const keyOf = (list: { id: string; parts: readonly unknown[] }[]) =>
      // Include a serialized size: the assistant text grows inside the same
      // part while a run streams, so the part count alone is not enough.
      list.map((m) => `${m.id}:${JSON.stringify(m.parts).length}`).join("|")
    const storedKey = keyOf(stored as { id: string; parts: readonly unknown[] }[])
    if (storedKey === lastSyncedRef.current) return
    lastSyncedRef.current = storedKey
    const currentKey = keyOf(messages as { id: string; parts: readonly unknown[] }[])
    if (storedKey !== currentKey) setMessages(stored as unknown as UIMessage[])
  }, [isNew, busy, storedMessages.data, messages, setMessages])

  // Reconnect to a run that is still going (reload / another tab). The replay
  // rebuilds the assistant message from its first chunk, so drop the partially
  // persisted copy first to avoid showing it twice.
  const resumeAttemptedRef = useRef(false)
  useEffect(() => {
    if (isNew) return
    if (!conversationRunning) {
      resumeAttemptedRef.current = false
      return
    }
    if (busy || resumeAttemptedRef.current) return
    resumeAttemptedRef.current = true
    // Prefer the persisted transcript (it includes a user message sent from
    // another tab), minus the partial assistant copy the replay will rebuild.
    const stored = storedMessages.data
    const base =
      stored && stored.length > 0 ? (stored as unknown as UIMessage[]) : messages
    setMessages(
      base.length > 0 && base[base.length - 1].role === "assistant" ? base.slice(0, -1) : base,
    )
    void resumeStream().catch((e) => toast.error(String(e)))
  }, [
    isNew,
    conversationRunning,
    busy,
    resumeStream,
    setMessages,
    storedMessages.data,
    messages,
  ])

  // Follow the transcript while a background run pushes new snapshots — but
  // only when the user is already pinned to the bottom.
  useEffect(() => {
    if (!atBottomRef.current) return
    const vp = viewportRef.current
    if (vp) vp.scrollTo({ top: vp.scrollHeight })
  }, [storedMessages.data])

  // Fetch once more when a background run finishes, in case the last poll
  // narrowly missed the final assistant message.
  const refetchStored = storedMessages.refetch
  const wasRunningRef = useRef(false)
  useEffect(() => {
    if (conversationRunning) {
      wasRunningRef.current = true
    } else if (wasRunningRef.current) {
      wasRunningRef.current = false
      void refetchStored()
    }
  }, [conversationRunning, refetchStored])
  // Shares the cache with WorkspacePanel; only used to decide whether the
  // header button should appear.
  const workspaceQuery = useQuery({
    queryKey: ["workspace", activeId],
    queryFn: () => workspaceApi.tree(activeId),
    enabled: !isNew,
    staleTime: 0,
  })
  const hasWorkspaceFiles = !isNew && (workspaceQuery.data?.length ?? 0) > 0
  const savedGroupId = isNew
    ? null
    : (conversationsQuery.data?.find((c) => c.id === activeId)?.group_id ?? null)
  // Until the freshly created row shows up in the list, remember the folder
  // the draft was started in so the header doesn't flash "Ungrouped".
  const pendingGroupId = isNew ? null : getPendingGroup(activeId)
  const desiredGroupId = isNew ? selectedFolderId : (savedGroupId ?? pendingGroupId)
  const desiredGroupExists =
    desiredGroupId === null ||
    groupsQuery.data === undefined ||
    groupsQuery.data.some((g) => g.id === desiredGroupId)
  const activeGroupId = desiredGroupExists ? desiredGroupId : null
  const activeGroup = groupsQuery.data?.find((g) => g.id === activeGroupId) ?? null

  // Conversations bucketed like the sidebar — used by the collapsed-header
  // folder menu to switch conversations.
  const conversationsByGroup = useMemo(() => {
    const byGroup = new Map<string, ConversationSummary[]>()
    const ungrouped: ConversationSummary[] = []
    const known = new Set((groupsQuery.data ?? []).map((g) => g.id))
    for (const c of conversationsQuery.data ?? []) {
      const groupId = c.group_id ?? getPendingGroup(c.id)
      if (groupId && known.has(groupId)) {
        const bucket = byGroup.get(groupId)
        if (bucket) bucket.push(c)
        else byGroup.set(groupId, [c])
      } else {
        ungrouped.push(c)
      }
    }
    return { byGroup, ungrouped }
  }, [conversationsQuery.data, groupsQuery.data])

  const openConversation = (c: ConversationSummary, groupId: string | null) => {
    setActiveFolder(groupId)
    navigate(`/chat/${c.id}`)
  }

  const renderConversationItem = (c: ConversationSummary, groupId: string | null) => {
    const selected = c.id === activeId
    return (
      <DropdownMenuItem
        key={c.id}
        aria-current={selected ? "page" : undefined}
        onSelect={() => openConversation(c, groupId)}
        className={cn(
          selected &&
            "bg-primary/10 font-medium text-foreground focus:bg-primary/15! focus:text-foreground",
        )}
      >
        <MessageSquareIcon className={cn(selected && "text-primary")} />
        <span className="max-w-56 truncate">{c.title}</span>
      </DropdownMenuItem>
    )
  }

  // The list query can run before the row exists (or return before the
  // create-time folder landed). Reconcile once the row is visible, then stop
  // tracking it.
  const reconciledGroups = useRef<Set<string>>(new Set())
  useEffect(() => {
    if (isNew) return
    const saved = conversationsQuery.data?.find((c) => c.id === activeId)?.group_id
    if (saved === undefined) return
    const pending = getPendingGroup(activeId)
    if (!pending) return
    if (saved === pending) {
      clearPendingGroup(activeId)
      reconciledGroups.current.delete(activeId)
      return
    }
    if (saved === null && !reconciledGroups.current.has(activeId)) {
      reconciledGroups.current.add(activeId)
      void conversationsApi
        .setGroup(activeId, pending)
        .then(() => {
          clearPendingGroup(activeId)
          reconciledGroups.current.delete(activeId)
          onConversationsChanged()
        })
        .catch((e) => {
          clearPendingGroup(activeId)
          toast.error(String(e))
        })
      return
    }
    // Another folder is authoritative (manual move) — stop tracking.
    clearPendingGroup(activeId)
  }, [isNew, activeId, conversationsQuery.data, onConversationsChanged])

  // The server inserts the conversation at the start of the run; refresh the
  // list as soon as the stream begins so the sidebar/header pick up the row
  // and its folder without waiting for the 15s poll.
  useEffect(() => {
    if (status !== "streaming") return
    onConversationsChanged()
  }, [status, onConversationsChanged])

  const moveGroup = useMutation({
    mutationFn: (groupId: string | null) => conversationsApi.setGroup(activeId, groupId),
    onSuccess: onConversationsChanged,
    onError: (e) => toast.error(String(e)),
  })

  const createFolder = useMutation({
    mutationFn: (name: string) => conversationGroupsApi.create(name),
    onSuccess: (created) => {
      void queryClient.invalidateQueries({ queryKey: ["conversation-groups"] })
      // Use the new folder for the next new chat.
      setActiveFolder(created.id)
      setFolderDialogOpen(false)
      toast.success(t("common.folder.created"))
    },
    onError: (e) => toast.error(String(e)),
  })

  const chooseGroup = (groupId: string | null) => {
    if (isNew) {
      setActiveFolder(groupId)
    } else {
      // Manual move wins over any pending create-time folder.
      clearPendingGroup(activeId)
      moveGroup.mutate(groupId)
    }
  }

  const handleViewportScroll = () => {
    const vp = viewportRef.current
    if (!vp) return
    atBottomRef.current = vp.scrollHeight - vp.scrollTop - vp.clientHeight < 48
  }

  // Keep the newest content in view during streaming — only when the user is
  // already pinned to the bottom. Runs on streamed message updates; never on
  // expand/collapse (those don't touch `messages`), so reading history while
  // opening a thought block can never be interrupted by a scroll.
  useEffect(() => {
    const vp = viewportRef.current
    if (!vp || !busy || !atBottomRef.current) return
    vp.scrollTo({ top: vp.scrollHeight })
  }, [messages, busy])

  const onApproval = (approvalId: string, approved: boolean) => {
    addToolApprovalResponse({ id: approvalId, approved })
  }

  // ----- Attachment upload / removal -----
  const onFilesSelected = async (fileList: FileList | null) => {
    const files = Array.from(fileList ?? [])
    if (files.length === 0) return
    if (fileInputRef.current) fileInputRef.current.value = ""
    setUploadingCount((n) => n + files.length)
    await Promise.all(
      files.map(async (file) => {
        try {
          const dto = await filesApi.upload(file)
          setPendingFiles((prev) => [...prev, dto])
        } catch (e) {
          toast.error(`${t("chat.uploadFailed")}: ${file.name}`, { description: String(e) })
        } finally {
          setUploadingCount((n) => n - 1)
        }
      }),
    )
  }

  const removePendingFile = async (dto: FileDto) => {
    setPendingFiles((prev) => prev.filter((f) => f.id !== dto.id))
    try {
      await filesApi.remove(dto.id)
    } catch {
      // Already-attached files can no longer be removed server-side; the chip
      // is gone locally either way.
    }
  }

  // Refresh the workspace tree when a run finishes (agent may have written files).
  const busyRef = useRef(busy)
  useEffect(() => {
    if (busyRef.current && !busy) {
      void queryClient.invalidateQueries({ queryKey: ["workspace", activeId] })
      void queryClient.invalidateQueries({ queryKey: ["feedback", activeId] })
      void queryClient.invalidateQueries({ queryKey: ["approvals", activeId] })
    }
    busyRef.current = busy
  }, [busy, activeId, queryClient])

  const cancel = useMutation({
    mutationFn: () => conversationsApi.cancel(activeId),
    onSuccess: () => onConversationsChanged(),
    onError: (e) => toast.error(String(e)),
  })

  const generateTitle = useMutation({
    mutationFn: () => conversationsApi.generateTitle(activeId),
    onSuccess: () => {
      toast.success(t("chat.titleGenerated"))
      onConversationsChanged()
    },
    onError: (e) => toast.error(String(e)),
  })

  const hasPendingApproval =
    busy &&
    (messages[messages.length - 1]?.parts.some(
      (p) => isToolUIPart(p) && (p as { state?: string }).state === "approval-requested",
    ) ??
      false)

  const canSend =
    !busy &&
    !conversationRunning &&
    uploadingCount === 0 &&
    (draft.trim().length > 0 || pendingFiles.length > 0)

  function submitMessage() {
    if (!canSend) return
    const text = draft.trim()
    const files = pendingFiles
    setDraft("")
    setPendingFiles([])
    if (isNew) {
      // Materialize the pre-allocated conversation id in the URL, then send —
      // the Chat instance is the same object, so the stream follows the URL.
      navigate(`/chat/${activeId}`, { replace: true })
      onConversationsChanged()
    }
    if (isNew && activeGroupId) setPendingGroup(activeId, activeGroupId)
    void sendMessage(
      {
        text,
        files: files.map((f) => ({
          type: "file" as const,
          mediaType: f.content_type,
          filename: f.filename,
          url: f.url,
        })),
      },
      // The backend only applies this while creating the conversation.
      isNew && activeGroupId ? { body: { group_id: activeGroupId } } : undefined,
    )
    // Sending always lands the user back at the bottom, even if they were
    // scrolled up reading. The streaming effect takes over from there.
    atBottomRef.current = true
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        viewportRef.current?.scrollTo({ top: Number.MAX_SAFE_INTEGER })
      })
    })
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault()
    submitMessage()
  }

  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault()
      formRef.current?.requestSubmit()
    }
  }

  return (
    <main className="flex h-full w-full min-w-0 flex-col">
      <header className="flex items-center justify-between gap-2 border-b px-4 py-2">
        {sidebarCollapsed ? (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="outline"
                size="sm"
                className="max-w-56 gap-1.5 text-foreground"
                title={t("common.folder.moveTo")}
              >
                <FolderIcon className="size-4 shrink-0" />
                <span className="truncate">{activeGroup?.name ?? t("common.folder.ungrouped")}</span>
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="start" className="min-w-48">
              <DropdownMenuSub>
                <DropdownMenuSubTrigger
                  onSelect={() => chooseGroup(null)}
                  className={cn(
                    activeGroupId === null &&
                      "bg-primary/10 font-medium text-foreground focus:bg-primary/15! data-[state=open]:bg-primary/15!",
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{t("common.folder.ungrouped")}</span>
                  <span
                    className={cn(
                      "rounded-full px-1.5 text-[10px] tabular-nums",
                      activeGroupId === null
                        ? "bg-primary/20 text-primary"
                        : "bg-muted text-muted-foreground",
                    )}
                  >
                    {conversationsByGroup.ungrouped.length}
                  </span>
                </DropdownMenuSubTrigger>
                <DropdownMenuSubContent className="max-h-64 min-w-52 overflow-y-auto">
                  {conversationsByGroup.ungrouped.length === 0 ? (
                    <DropdownMenuItem disabled>{t("common.folder.empty")}</DropdownMenuItem>
                  ) : (
                    conversationsByGroup.ungrouped.map((c) => renderConversationItem(c, null))
                  )}
                </DropdownMenuSubContent>
              </DropdownMenuSub>
              {(groupsQuery.data ?? []).map((g) => {
                const items = conversationsByGroup.byGroup.get(g.id) ?? []
                const selectedGroup = activeGroupId === g.id
                return (
                  <DropdownMenuSub key={g.id}>
                    <DropdownMenuSubTrigger
                      onSelect={() => chooseGroup(g.id)}
                      className={cn(
                        selectedGroup &&
                          "bg-primary/10 font-medium text-foreground focus:bg-primary/15! data-[state=open]:bg-primary/15!",
                      )}
                    >
                      <FolderIcon className={cn(selectedGroup && "text-primary")} />
                      <span className="min-w-0 flex-1 truncate">{g.name}</span>
                      <span
                        className={cn(
                          "rounded-full px-1.5 text-[10px] tabular-nums",
                          selectedGroup
                            ? "bg-primary/20 text-primary"
                            : "bg-muted text-muted-foreground",
                        )}
                      >
                        {items.length}
                      </span>
                    </DropdownMenuSubTrigger>
                    <DropdownMenuSubContent className="max-h-64 min-w-52 overflow-y-auto">
                      {items.length === 0 ? (
                        <DropdownMenuItem disabled>{t("common.folder.empty")}</DropdownMenuItem>
                      ) : (
                        items.map((c) => renderConversationItem(c, g.id))
                      )}
                    </DropdownMenuSubContent>
                  </DropdownMenuSub>
                )
              })}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => setFolderDialogOpen(true)}>
                <FolderPlusIcon /> {t("common.folder.newShort")}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        ) : (
          <div />
        )}
        <div className="flex items-center gap-3 text-sm text-muted-foreground">
          <Button
            variant="outline"
            size="sm"
            className="gap-1.5 text-foreground"
            onClick={() => generateTitle.mutate()}
            disabled={isNew || busy || generateTitle.isPending}
            title={t("chat.generateTitle")}
          >
            <SparklesIcon className={cn("size-4", generateTitle.isPending && "animate-pulse")} />
            <span className="hidden sm:inline">{t("chat.generateTitle")}</span>
          </Button>
          {hasWorkspaceFiles && (
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5 text-foreground"
              onClick={() => setWorkspaceOpen(true)}
              title={t("chat.workspace.title")}
            >
              <FolderOpenIcon className="size-4" />
              <span className="hidden sm:inline">{t("chat.workspace.title")}</span>
            </Button>
          )}
        </div>
      </header>

      <MessageScrollerProvider autoScroll={false}>
        <MessageScroller className="min-h-0 flex-1">
          <MessageScrollerViewport ref={viewportRef} onScroll={handleViewportScroll}>
            <MessageScrollerContent className="mx-auto w-full max-w-3xl gap-0 px-4 py-6">
              {messages.length === 0 ? (
                <EmptyState
                  onPick={(text) => {
                    if (isNew) {
                      navigate(`/chat/${activeId}`, { replace: true })
                      onConversationsChanged()
                    }
                    if (isNew && activeGroupId) setPendingGroup(activeId, activeGroupId)
                    void sendMessage(
                      { text },
                      isNew && activeGroupId
                        ? { body: { group_id: activeGroupId } }
                        : undefined,
                    )
                  }}
                />
              ) : (
                <>
                  {messages.map((message, i) => {
                    const isLast = i === messages.length - 1
                    const workingHere = showWorking && isLast
                    // Skip metadata-only messages: the scroller item's padding
                    // would otherwise leave a gap between adjacent parts.
                    if (!hasRenderableOutput(message) && !workingHere) return null
                    // Consecutive assistant messages are steps of the same turn
                    // (tool call → thought → …): no space between them.
                    const tightAbove =
                      message.role === "assistant" && messages[i - 1]?.role === "assistant"
                    const tightBelow =
                      message.role === "assistant" && messages[i + 1]?.role === "assistant"
                    return (
                      <MessageScrollerItem
                        key={message.id}
                        messageId={message.id}
                        className={cn("py-3", tightAbove && "pt-2", tightBelow && "pb-0")}
                      >
                        <MessageRenderer
                          message={message}
                          streaming={busy && isLast}
                          conversationId={activeId}
                          feedback={feedbackQuery.data}
                          approvals={approvalsQuery.data}
                          onApproval={onApproval}
                        />
                        {showWorking && isLast && message.role === "assistant" && (
                          <WorkingIndicator className={hasRenderableOutput(message) ? "mt-2" : undefined} />
                        )}
                      </MessageScrollerItem>
                    )
                  })}
                  {showWorking && messages[messages.length - 1]?.role !== "assistant" && (
                    <MessageScrollerItem className="py-3">
                      <WorkingIndicator />
                    </MessageScrollerItem>
                  )}
                  {conversationRunning && !busy && (
                    <MessageScrollerItem className="py-3">
                      <div className="flex items-center gap-2 text-sm text-muted-foreground">
                        <Spinner className="size-4" />
                        {t("chat.runningInBackground")}
                      </div>
                    </MessageScrollerItem>
                  )}
                </>
              )}
            </MessageScrollerContent>
          </MessageScrollerViewport>
        </MessageScroller>
      </MessageScrollerProvider>

      <div className="mx-auto w-full max-w-3xl px-4 pb-4">
        <form ref={formRef} onSubmit={onSubmit}>
          {(pendingFiles.length > 0 || uploadingCount > 0) && (
            <div className="mb-2 flex flex-wrap gap-2">
              {pendingFiles.map((f) => (
                <span
                  key={f.id}
                  className="flex max-w-60 items-center gap-1.5 rounded-full border bg-muted/50 py-1 pr-1.5 pl-3 text-xs"
                >
                  <span className="truncate">{f.filename}</span>
                  <button
                    type="button"
                    className="rounded-full p-0.5 text-muted-foreground hover:bg-muted"
                    onClick={() => void removePendingFile(f)}
                    aria-label={t("chat.removeAttachment")}
                    title={t("chat.removeAttachment")}
                  >
                    <XIcon className="size-3" />
                  </button>
                </span>
              ))}
              {uploadingCount > 0 && (
                <span className="flex items-center gap-1.5 rounded-full border bg-muted/50 px-3 py-1 text-xs text-muted-foreground">
                  <Spinner className="size-3" />
                  {t("chat.uploading")}
                </span>
              )}
            </div>
          )}
          <InputGroup>
            <input
              ref={fileInputRef}
              type="file"
              multiple
              className="hidden"
              onChange={(e) => void onFilesSelected(e.target.files)}
              aria-hidden
            />
            <InputGroupTextarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={onKeyDown}
              placeholder={t("chat.placeholder")}
              aria-label={t("chat.placeholder")}
              className="min-h-14"
            />
            <InputGroupAddon align="block-end" className="justify-between gap-2">
              {/* Agent + model pickers and context meter live in the composer. */}
              <div className="flex min-w-0 items-center gap-1">
                <AgentBadge />
                <ModelSelector
                  value={modelId}
                  effort={effort}
                  onChange={onModelChange}
                  onEffortChange={onEffortChange}
                />
                <ContextMeter messages={messages} />
              </div>
              {/* Action cluster — compact 32px footprint, icon buttons equal. */}
              <div className="flex shrink-0 items-center gap-2">
                {(busy || conversationRunning) && !hasPendingApproval && (
                  <InputGroupButton
                    variant="secondary"
                    className="h-8 rounded-md px-3"
                    type="button"
                    onClick={() => {
                      if (busy) stop()
                      cancel.mutate()
                    }}
                  >
                    <CircleStopIcon /> {t("chat.stop")}
                  </InputGroupButton>
                )}
                <InputGroupButton
                  variant="secondary"
                  size="icon-xs"
                  className="size-8 rounded-md border border-primary/30 bg-primary/10 text-foreground hover:bg-primary/15"
                  type="button"
                  disabled={busy}
                  onClick={() => fileInputRef.current?.click()}
                  aria-label={t("chat.attach")}
                  title={t("chat.attach")}
                >
                  <PaperclipIcon className="size-3.5" />
                </InputGroupButton>
                <Button
                  type="submit"
                  size="icon"
                  className="size-8"
                  disabled={!canSend}
                  aria-label={t("chat.send")}
                >
                  <ArrowUpIcon className="size-3.5" />
                </Button>
              </div>
            </InputGroupAddon>
          </InputGroup>
        </form>
      </div>

      <WorkspacePanel
        open={workspaceOpen}
        onOpenChange={setWorkspaceOpen}
        conversationId={isNew ? undefined : activeId}
      />

      <GroupNameDialog
        open={folderDialogOpen}
        onOpenChange={setFolderDialogOpen}
        title={t("common.folder.new")}
        confirmLabel={t("common.create")}
        pending={createFolder.isPending}
        onSubmit={(name) => createFolder.mutate(name)}
      />
    </main>
  )
}

/** Any part that renders something the user can see. */
function hasRenderableOutput(message: UIMessage): boolean {
  return message.parts.some((p) => {
    if (p.type === "text" || p.type === "reasoning") return p.text.trim().length > 0
    if (p.type === "file") return true
    if (isToolUIPart(p)) return true
    if (p.type.startsWith("data-")) {
      const kind = p.type.slice("data-".length)
      return kind !== "conversation" && kind !== "run" && kind !== "usage"
    }
    return false
  })
}

function WorkingIndicator({ className }: { className?: string }) {
  const { t } = useTranslation()
  return (
    <div className={cn("flex items-center gap-2 text-sm text-muted-foreground", className)}>
      <Spinner className="size-4" />
      <span>{t("chat.working")}</span>
    </div>
  )
}

function EmptyState({ onPick }: { onPick: (text: string) => void }) {
  const { t } = useTranslation()
  const suggestions = [t("chat.suggest.time")]
  return (
    <div className="flex flex-col items-center gap-4 py-20 text-center">
      <h1 className="text-2xl font-semibold">{t("chat.empty.title")}</h1>
      <p className="text-muted-foreground">{t("chat.empty.subtitle")}</p>
      <div className="mt-2 flex flex-wrap justify-center gap-2">
        {suggestions.map((s) => (
          <Button key={s} variant="outline" size="sm" onClick={() => onPick(s)}>
            {s}
          </Button>
        ))}
      </div>
    </div>
  )
}
