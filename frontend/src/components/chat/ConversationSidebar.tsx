import { useEffect, useMemo, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { useNavigate, useParams } from "react-router-dom"
import {
  ChevronRightIcon,
  CircleStopIcon,
  FolderIcon,
  FolderPlusIcon,
  LogOutIcon,
  LoaderCircleIcon,
  MessageSquarePlusIcon,
  MoreHorizontalIcon,
  PanelLeftCloseIcon,
  PanelLeftOpenIcon,
  PencilIcon,
  SettingsIcon,
  ShieldCheckIcon,
  SparklesIcon,
  TrashIcon,
  UserIcon,
} from "lucide-react"
import { toast } from "sonner"

import { conversationGroupsApi, conversationsApi } from "@/lib/api/endpoints"
import { useAuth } from "@/lib/auth/AuthContext"
import { getPendingGroup, setActiveFolder, useActiveFolder } from "@/lib/chat/folder-selection"
import type { ConversationGroup, ConversationSummary, PublicUser } from "@/lib/types"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { UserAvatar } from "@/components/shared/UserAvatar"
import { Separator } from "@/components/ui/separator"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import { ConfirmDialog } from "@/components/shared/ConfirmDialog"
import { GroupNameDialog } from "@/components/shared/GroupNameDialog"
import { Spinner } from "@/components/shared/Spinner"

const COLLAPSED_KEY = "assistant_sidebar_collapsed_groups"
const UNGROUPED_KEY = "__ungrouped"

type FolderDialogState =
  | { mode: "create"; moveConversationId?: string }
  | { mode: "rename"; group: ConversationGroup }

function readCollapsed(): string[] {
  try {
    const raw = localStorage.getItem(COLLAPSED_KEY)
    const parsed: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === "string") : []
  } catch {
    return []
  }
}

export function ConversationSidebar({
  collapsed,
  onToggleCollapsed,
}: {
  collapsed: boolean
  onToggleCollapsed: () => void
}) {
  const { t } = useTranslation()
  const { user, logout } = useAuth()
  const navigate = useNavigate()
  const { conversationId } = useParams()
  const queryClient = useQueryClient()

  // Conversation pending deletion confirmation (framework dialog, not window.confirm).
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; title: string } | null>(null)
  // Sidebar row whose context menu is currently open — highlighted so the
  // menu target is obvious even when it is not the open conversation.
  const [menuTargetId, setMenuTargetId] = useState<string | null>(null)
  const [collapsedFolders, setCollapsedFolders] = useState<string[]>(readCollapsed)
  // Folder new chats go into ("Ungrouped" when null) — shared with the chat
  // header so a draft shows the same destination.
  const activeFolderId = useActiveFolder()
  const [folderDialog, setFolderDialog] = useState<FolderDialogState | null>(null)
  const [deleteFolderTarget, setDeleteFolderTarget] = useState<ConversationGroup | null>(null)
  const [stopAllOpen, setStopAllOpen] = useState(false)

  const conversations = useQuery({
    queryKey: ["conversations"],
    queryFn: () => conversationsApi.list(),
    // Poll faster while a run is active so the activity indicator is timely.
    refetchInterval: (query) =>
      (query.state.data ?? []).some((c) => c.status === "running") ? 3_000 : 15_000,
  })

  const groups = useQuery({
    queryKey: ["conversation-groups"],
    queryFn: () => conversationGroupsApi.list(),
  })

  useEffect(() => {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify(collapsedFolders))
  }, [collapsedFolders])

  const invalidateConversations = () =>
    void queryClient.invalidateQueries({ queryKey: ["conversations"] })
  const invalidateGroups = () =>
    void queryClient.invalidateQueries({ queryKey: ["conversation-groups"] })

  const runningIds = (conversations.data ?? [])
    .filter((c) => c.status === "running")
    .map((c) => c.id)

  const stopAll = useMutation({
    mutationFn: (ids: string[]) => Promise.all(ids.map((id) => conversationsApi.cancel(id))),
    onSuccess: () => {
      invalidateConversations()
      setStopAllOpen(false)
      toast.success(t("sidebar.stopAllDone"))
    },
    onError: (e) => toast.error(String(e)),
  })

  // Runs continue on the server after the tab closes (that's the default).
  // Still warn before leaving while something is running, so the user can
  // come back and stop it if that was not intended.
  useEffect(() => {
    if (runningIds.length === 0) return
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      e.preventDefault()
      e.returnValue = ""
    }
    window.addEventListener("beforeunload", onBeforeUnload)
    return () => window.removeEventListener("beforeunload", onBeforeUnload)
  }, [runningIds.length])

  const remove = useMutation({
    mutationFn: (id: string) => conversationsApi.remove(id),
    onSuccess: (_, deletedId) => {
      invalidateConversations()
      // Leave the chat surface only when the deleted one is the open one —
      // deleting any other conversation must not disturb the current view.
      if (deletedId === conversationId) navigate("/chat")
    },
    onError: (e) => toast.error(String(e)),
  })

  const generateTitle = useMutation({
    mutationFn: (id: string) => conversationsApi.generateTitle(id),
    onSuccess: () => {
      invalidateConversations()
      toast.success(t("sidebar.titleGenerated"))
    },
    onError: (e) => toast.error(String(e)),
  })

  const setGroup = useMutation({
    mutationFn: ({ id, groupId }: { id: string; groupId: string | null }) =>
      conversationsApi.setGroup(id, groupId),
    onSuccess: invalidateConversations,
    onError: (e) => toast.error(String(e)),
  })

  const createGroup = useMutation({
    mutationFn: async ({
      name,
      moveConversationId,
    }: {
      name: string
      moveConversationId?: string
    }) => {
      const created = await conversationGroupsApi.create(name)
      if (moveConversationId) {
        await conversationsApi.setGroup(moveConversationId, created.id)
      }
      return created
    },
    onSuccess: () => {
      invalidateGroups()
      invalidateConversations()
      setFolderDialog(null)
      toast.success(t("common.folder.created"))
    },
    onError: (e) => toast.error(String(e)),
  })

  const renameGroup = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) =>
      conversationGroupsApi.rename(id, name),
    onSuccess: () => {
      invalidateGroups()
      setFolderDialog(null)
      toast.success(t("common.folder.renamed"))
    },
    onError: (e) => toast.error(String(e)),
  })

  const removeGroup = useMutation({
    mutationFn: (id: string) => conversationGroupsApi.remove(id),
    onSuccess: () => {
      invalidateGroups()
      invalidateConversations()
      setDeleteFolderTarget(null)
      toast.success(t("common.folder.deleted"))
    },
    onError: (e) => toast.error(String(e)),
  })

  const sections = useMemo(() => {
    const list = conversations.data ?? []
    const folders = groups.data ?? []
    const known = new Set(folders.map((g) => g.id))
    const byGroup = new Map<string, ConversationSummary[]>()
    const ungrouped: ConversationSummary[] = []
    for (const c of list) {
      // A just-created conversation may not carry its folder in an early list
      // response; the pending value keeps it in the intended section.
      const groupId = c.group_id ?? getPendingGroup(c.id)
      if (groupId && known.has(groupId)) {
        const bucket = byGroup.get(groupId)
        if (bucket) bucket.push(c)
        else byGroup.set(groupId, [c])
      } else {
        ungrouped.push(c)
      }
    }
    return { folders, byGroup, ungrouped }
  }, [conversations.data, groups.data])

  // Ignore a stale stored id (folder deleted in another tab).
  const activeFolder = sections.folders.find((g) => g.id === activeFolderId) ?? null

  // Already composing a new chat: selecting a folder updates its destination
  // in place, so there is nothing to navigate to.
  const newChat = () => {
    if (conversationId) {
      navigate(activeFolder ? `/chat?folder=${activeFolder.id}` : "/chat")
    }
  }

  const toggleSection = (key: string, open: boolean) => {
    setCollapsedFolders((prev) => {
      if (open) return prev.filter((k) => k !== key)
      return prev.includes(key) ? prev : [...prev, key]
    })
  }

  const renderRow = (c: ConversationSummary) => (
    <li key={c.id}>
      <ContextMenu onOpenChange={(open) => setMenuTargetId(open ? c.id : null)}>
        <ContextMenuTrigger asChild>
          <button
            className={cn(
              "relative flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-sidebar-accent",
              // Selected: primary-tinted background + left rail —
              // bg-sidebar-accent alone is too subtle to spot.
              c.id === conversationId &&
                "bg-primary/10 font-medium text-foreground before:absolute before:inset-y-1 before:left-0 before:w-0.5 before:rounded-full before:bg-primary",
              // Context-menu target: primary tint + ring while its
              // menu is open (the open conversation keeps its rail).
              menuTargetId === c.id && "bg-primary/15 font-medium ring-2 ring-primary/50",
            )}
            onClick={() => {
              const groupId = c.group_id ?? getPendingGroup(c.id)
              const folderId =
                groupId && sections.folders.some((g) => g.id === groupId) ? groupId : null
              setActiveFolder(folderId)
              navigate(`/chat/${c.id}`)
            }}
            title={c.title}
          >
            {c.status === "running" ? (
              <span
                className="shrink-0 text-primary"
                title={t("sidebar.running")}
                aria-label={t("sidebar.running")}
              >
                <LoaderCircleIcon className="size-3.5 animate-spin" />
              </span>
            ) : (
              // Keep titles aligned whether or not the row is running.
              <span className="w-3.5 shrink-0" aria-hidden />
            )}
            <span className="min-w-0 flex-1 truncate">{c.title}</span>
          </button>
        </ContextMenuTrigger>
        <ContextMenuContent>
          <ContextMenuSub>
            <ContextMenuSubTrigger>
              <FolderIcon /> {t("common.folder.move")}
            </ContextMenuSubTrigger>
            <ContextMenuSubContent>
              {sections.folders.length > 0 && (
                <ContextMenuItem
                  disabled={c.group_id === null}
                  onSelect={() => setGroup.mutate({ id: c.id, groupId: null })}
                >
                  {t("common.folder.ungrouped")}
                </ContextMenuItem>
              )}
              {sections.folders.map((g) => (
                <ContextMenuItem
                  key={g.id}
                  disabled={c.group_id === g.id}
                  onSelect={() => setGroup.mutate({ id: c.id, groupId: g.id })}
                >
                  <FolderIcon /> {g.name}
                </ContextMenuItem>
              ))}
              {sections.folders.length > 0 && <ContextMenuSeparator />}
              <ContextMenuItem
                onSelect={() => setFolderDialog({ mode: "create", moveConversationId: c.id })}
              >
                <FolderPlusIcon /> {t("common.folder.new")}
              </ContextMenuItem>
            </ContextMenuSubContent>
          </ContextMenuSub>
          <ContextMenuItem
            onSelect={() => generateTitle.mutate(c.id)}
            disabled={generateTitle.isPending && generateTitle.variables === c.id}
          >
            <SparklesIcon /> {t("sidebar.generateTitle")}
          </ContextMenuItem>
          <ContextMenuItem
            variant="destructive"
            onSelect={() => setDeleteTarget({ id: c.id, title: c.title })}
          >
            <TrashIcon /> {t("sidebar.delete")}
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
    </li>
  )

  const renderSection = (
    key: string,
    name: string,
    items: ConversationSummary[],
    group?: ConversationGroup,
  ) => {
    const open = !collapsedFolders.includes(key)
    const active = group ? group.id === activeFolder?.id : activeFolder === null
    return (
      <Collapsible key={key} open={open} onOpenChange={(next) => toggleSection(key, next)}>
        <div
          className={cn(
            "flex items-center gap-0.5 rounded-md pr-0.5 pl-1 transition-colors",
            // Active must keep its tint while hovered — a plain `hover:` class
            // would otherwise override the active background.
            active ? "bg-primary/10 hover:bg-primary/15" : "hover:bg-sidebar-accent/60",
          )}
        >
          <CollapsibleTrigger asChild>
            <button
              className="shrink-0 rounded p-1 text-muted-foreground hover:text-foreground"
              aria-label={open ? t("common.folder.collapse") : t("common.folder.expand")}
            >
              <ChevronRightIcon
                className={cn("size-4 transition-transform", open && "rotate-90")}
              />
            </button>
          </CollapsibleTrigger>
          <button
            onClick={() => setActiveFolder(group?.id ?? null)}
            aria-pressed={active}
            title={t("common.folder.selectHint")}
            className={cn(
              "flex min-w-0 flex-1 items-center gap-1.5 py-1.5 text-left text-xs font-medium tracking-wide text-muted-foreground uppercase",
              active && "font-semibold text-foreground",
            )}
          >
            <FolderIcon className={cn("size-4 shrink-0", active && "text-primary")} />
            <span className="truncate">{name}</span>
            <span
              className={cn(
                "ml-auto shrink-0 rounded-full px-1.5 text-[10px] tabular-nums",
                active ? "bg-primary/20 text-primary" : "bg-muted",
              )}
            >
              {items.length}
            </span>
          </button>
          {group && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  className="shrink-0 text-muted-foreground"
                  aria-label={t("common.folder.rename")}
                >
                  <MoreHorizontalIcon />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onSelect={() => setFolderDialog({ mode: "rename", group })}>
                  <PencilIcon /> {t("common.rename")}
                </DropdownMenuItem>
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() => setDeleteFolderTarget(group)}
                >
                  <TrashIcon /> {t("common.delete")}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
        </div>
        <CollapsibleContent>
          <div className="pt-1">
            {items.length === 0 ? (
              <p className="px-2 py-1 text-xs text-muted-foreground">{t("common.folder.empty")}</p>
            ) : (
              <ul className="grid gap-1">{items.map(renderRow)}</ul>
            )}
          </div>
        </CollapsibleContent>
      </Collapsible>
    )
  }

  const hasFolders = sections.folders.length > 0

  if (collapsed) {
    return (
      <aside className="flex h-full w-full flex-col items-center gap-1.5 overflow-hidden border-r bg-sidebar py-2 text-sidebar-foreground">
        <TooltipProvider delayDuration={200}>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon-lg"
                variant="outline"
                onClick={onToggleCollapsed}
                aria-label={t("sidebar.expand")}
              >
                <PanelLeftOpenIcon className="size-5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">{t("sidebar.expand")}</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon-lg"
                variant="outline"
                onClick={newChat}
                aria-label={t("chat.newChat")}
              >
                <MessageSquarePlusIcon className="size-5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">{t("chat.newChat")}</TooltipContent>
          </Tooltip>
          {runningIds.length > 0 && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="icon-lg"
                  variant="outline"
                  className="text-destructive"
                  onClick={() => setStopAllOpen(true)}
                  aria-label={t("sidebar.stopAll")}
                >
                  <CircleStopIcon className="size-5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="right">{t("sidebar.stopAll")}</TooltipContent>
            </Tooltip>
          )}
          <div className="my-2 h-px w-8 bg-border" />
          <div className="flex-1" />
          {user?.role === "admin" && (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="icon-lg"
                  variant="outline"
                  onClick={() => navigate("/admin")}
                  aria-label={t("sidebar.admin")}
                >
                  <ShieldCheckIcon className="size-5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="right">{t("sidebar.admin")}</TooltipContent>
            </Tooltip>
          )}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                size="icon-lg"
                variant="outline"
                onClick={() => navigate("/settings")}
                aria-label={t("sidebar.settings")}
              >
                <SettingsIcon className="size-5" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="right">{t("sidebar.settings")}</TooltipContent>
          </Tooltip>
          {user && (
            <div className="mt-1">
              <UserMenu user={user} collapsed onNavigate={navigate} onLogout={logout} />
            </div>
          )}
        </TooltipProvider>
      </aside>
    )
  }

  return (
    <aside className="flex h-full w-full flex-col border-r bg-sidebar text-sidebar-foreground">
      <div className="min-h-0 flex-1 overflow-y-auto p-2">
        <div className="flex items-center justify-between gap-1 px-2 pb-1">
          <p className="min-w-0 truncate text-xs font-medium tracking-wide text-muted-foreground uppercase">
            {t("sidebar.conversations")}
          </p>
          <div className="flex shrink-0 items-center">
            {runningIds.length > 0 && (
              <Button
                size="icon-sm"
                variant="ghost"
                className="text-destructive"
                title={t("sidebar.stopAll")}
                aria-label={t("sidebar.stopAll")}
                onClick={() => setStopAllOpen(true)}
              >
                <CircleStopIcon className="size-4" />
              </Button>
            )}
            <Button
              size="icon-sm"
              variant="ghost"
              className="text-muted-foreground"
              title={t("chat.newChat")}
              aria-label={t("chat.newChat")}
              onClick={newChat}
            >
              <MessageSquarePlusIcon className="size-4" />
            </Button>
            <Button
              size="icon-sm"
              variant="ghost"
              className="text-muted-foreground"
              title={t("common.folder.new")}
              aria-label={t("common.folder.new")}
              onClick={() => setFolderDialog({ mode: "create" })}
            >
              <FolderPlusIcon className="size-4" />
            </Button>
            <Button
              size="icon-sm"
              variant="ghost"
              className="text-muted-foreground"
              title={t("sidebar.collapse")}
              aria-label={t("sidebar.collapse")}
              onClick={onToggleCollapsed}
            >
              <PanelLeftCloseIcon className="size-4" />
            </Button>
          </div>
        </div>
        {conversations.isLoading || groups.isLoading ? (
          <div className="flex justify-center py-6">
            <Spinner />
          </div>
        ) : hasFolders ? (
          <div className="grid gap-2">
            {sections.folders.map((g) =>
              renderSection(g.id, g.name, sections.byGroup.get(g.id) ?? [], g),
            )}
            {sections.ungrouped.length > 0 &&
              renderSection(UNGROUPED_KEY, t("common.folder.ungrouped"), sections.ungrouped)}
          </div>
        ) : (
          <ul className="grid gap-1">
            {(conversations.data ?? []).map(renderRow)}
            {conversations.data?.length === 0 && (
              <li className="px-2 py-6 text-center text-sm text-muted-foreground">—</li>
            )}
          </ul>
        )}
      </div>
      <Separator />
      <div className="grid gap-0.5 p-2 text-sm">
        {user?.role === "admin" && (
          <Button variant="ghost" className="justify-start" onClick={() => navigate("/admin")}>
            <ShieldCheckIcon /> {t("sidebar.admin")}
          </Button>
        )}
        <Button variant="ghost" className="justify-start" onClick={() => navigate("/settings")}>
          <SettingsIcon /> {t("sidebar.settings")}
        </Button>
        <div className="mt-1 flex items-center justify-between gap-2 border-t pt-2">
          {user && (
            <UserMenu user={user} collapsed={false} onNavigate={navigate} onLogout={logout} />
          )}
        </div>
      </div>

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null)
        }}
        title={t("sidebar.deleteConfirm")}
        description={deleteTarget?.title}
        onConfirm={() => {
          if (deleteTarget) remove.mutate(deleteTarget.id)
        }}
      />
      <ConfirmDialog
        open={deleteFolderTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteFolderTarget(null)
        }}
        title={t("common.folder.deleteConfirm")}
        description={t("common.folder.deleteDescription")}
        onConfirm={() => {
          if (deleteFolderTarget) removeGroup.mutate(deleteFolderTarget.id)
        }}
      />
      <ConfirmDialog
        open={stopAllOpen}
        onOpenChange={setStopAllOpen}
        title={t("sidebar.stopAllConfirm")}
        description={t("sidebar.stopAllDescription", { count: runningIds.length })}
        confirmLabel={t("sidebar.stopAll")}
        onConfirm={() => stopAll.mutate(runningIds)}
      />
      <GroupNameDialog
        open={folderDialog !== null}
        onOpenChange={(open) => {
          if (!open) setFolderDialog(null)
        }}
        title={
          folderDialog?.mode === "rename" ? t("common.folder.rename") : t("common.folder.new")
        }
        initialName={folderDialog?.mode === "rename" ? folderDialog.group.name : ""}
        confirmLabel={
          folderDialog?.mode === "rename" ? t("common.save") : t("common.create")
        }
        pending={createGroup.isPending || renameGroup.isPending}
        onSubmit={(name) => {
          if (!folderDialog) return
          if (folderDialog.mode === "create") {
            createGroup.mutate({ name, moveConversationId: folderDialog.moveConversationId })
          } else {
            renameGroup.mutate({ id: folderDialog.group.id, name })
          }
        }}
      />
    </aside>
  )
}

/** Avatar menu used by both the expanded sidebar and the collapsed rail. */
function UserMenu({
  user,
  collapsed,
  onNavigate,
  onLogout,
}: {
  user: PublicUser
  collapsed: boolean
  onNavigate: (to: string) => void
  onLogout: () => void
}) {
  const { t } = useTranslation()
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          title={t("profile.title")}
          className={cn(
            "flex min-w-0 items-center gap-2 rounded-md text-left text-sm",
            collapsed
              ? "justify-center border bg-background p-1 shadow-xs hover:bg-accent"
              : "px-1 py-1 hover:bg-sidebar-accent",
          )}
        >
          <UserAvatar user={user} size="default" />
          {!collapsed && <span className="truncate">{user.name || user.email}</span>}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        align={collapsed ? "center" : "start"}
        side={collapsed ? "right" : "top"}
        className="min-w-56"
      >
        <DropdownMenuLabel>
          <div className="grid gap-0.5">
            {user.name && <span className="font-medium">{user.name}</span>}
            <span className="text-xs font-normal text-muted-foreground">{user.email}</span>
            <span className="text-xs font-normal text-muted-foreground capitalize">{user.role}</span>
          </div>
        </DropdownMenuLabel>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => onNavigate("/profile")}>
          <UserIcon /> {t("profile.title")}
        </DropdownMenuItem>
        <DropdownMenuItem variant="destructive" onSelect={onLogout}>
          <LogOutIcon /> {t("sidebar.logout")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
