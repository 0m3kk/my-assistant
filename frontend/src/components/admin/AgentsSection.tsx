import { useMemo, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  ChevronRightIcon,
  FolderIcon,
  FolderPlusIcon,
  MoreHorizontalIcon,
  PencilIcon,
  PlusIcon,
  TrashIcon,
} from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import type { Agent, AgentGroup } from "@/lib/types"
import { cn } from "@/lib/utils"
import { ApiError } from "@/lib/api/client"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ToolsSectionPicker } from "./ToolsSection"
import { AgentHistoryButton } from "./AgentHistoryButton"
import { ConfirmDialog } from "@/components/shared/ConfirmDialog"
import { GroupNameDialog } from "@/components/shared/GroupNameDialog"

const UNGROUPED = "__ungrouped"
const BUILTIN = "__builtin"

type FolderDialogState = { mode: "create" } | { mode: "rename"; group: AgentGroup }

export function AgentsSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<Agent | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Agent | null>(null)
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [folderDialog, setFolderDialog] = useState<FolderDialogState | null>(null)
  const [deleteFolderTarget, setDeleteFolderTarget] = useState<AgentGroup | null>(null)

  const agents = useQuery({ queryKey: ["admin", "agents"], queryFn: () => adminApi.agents.list() })
  const groups = useQuery({
    queryKey: ["admin", "agent-groups"],
    queryFn: () => adminApi.agentGroups.list(),
  })

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ["admin", "agents"] })
  const invalidateGroups = () =>
    void queryClient.invalidateQueries({ queryKey: ["admin", "agent-groups"] })

  const remove = useMutation({
    mutationFn: (id: string) => adminApi.agents.remove(id),
    onSuccess: (_, deletedId) => {
      // Close the editor only when the deleted agent is the one being edited —
      // deleting another agent must not disturb the open editor.
      if (selected?.id === deletedId) setSelected(null)
      invalidate()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const assign = useMutation({
    mutationFn: ({ id, groupId }: { id: string; groupId: string | null }) =>
      adminApi.agents.update(id, { group_id: groupId }),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const createGroup = useMutation({
    mutationFn: (name: string) => adminApi.agentGroups.create(name),
    onSuccess: () => {
      invalidateGroups()
      setFolderDialog(null)
      toast.success(t("common.folder.created"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const renameGroup = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => adminApi.agentGroups.update(id, name),
    onSuccess: () => {
      invalidateGroups()
      setFolderDialog(null)
      toast.success(t("common.folder.renamed"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const removeGroup = useMutation({
    mutationFn: (id: string) => adminApi.agentGroups.remove(id),
    onSuccess: () => {
      invalidateGroups()
      invalidate()
      setDeleteFolderTarget(null)
      toast.success(t("common.folder.deleted"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const sections = useMemo(() => {
    const list = agents.data?.agents ?? []
    const folders = groups.data?.groups ?? []
    const known = new Set(folders.map((g) => g.id))
    const byGroup = new Map<string, Agent[]>()
    const builtin: Agent[] = []
    const ungrouped: Agent[] = []
    for (const agent of list) {
      if (agent.group_id && known.has(agent.group_id)) {
        const bucket = byGroup.get(agent.group_id)
        if (bucket) bucket.push(agent)
        else byGroup.set(agent.group_id, [agent])
      } else if (agent.is_builtin) {
        builtin.push(agent)
      } else {
        ungrouped.push(agent)
      }
    }
    // `general` is the default agent and always leads its section.
    const byImportance = (a: Agent, b: Agent) => {
      const aGeneral = a.slug === "general" ? 0 : 1
      const bGeneral = b.slug === "general" ? 0 : 1
      if (aGeneral !== bGeneral) return aGeneral - bGeneral
      return a.name.localeCompare(b.name)
    }
    for (const bucket of byGroup.values()) bucket.sort(byImportance)
    builtin.sort(byImportance)
    ungrouped.sort(byImportance)
    return { folders, byGroup, builtin, ungrouped }
  }, [agents.data, groups.data])

  const toggleSection = (key: string, openSection: boolean) => {
    setCollapsed((prev) => {
      if (openSection) return prev.filter((k) => k !== key)
      return prev.includes(key) ? prev : [...prev, key]
    })
  }

  const renderCard = (a: Agent) => (
    <div
      key={a.id}
      className={cn(
        "flex items-start justify-between gap-2 rounded-lg border p-4 transition-colors",
        selected?.id === a.id && "border-primary",
      )}
    >
      <button className="min-w-0 flex-1 text-left" onClick={() => setSelected(a)}>
        <p className="font-medium">
          {a.name} <span className="text-xs text-muted-foreground">/{a.slug}</span>
        </p>
        <p className="text-sm text-muted-foreground">{a.description}</p>
        <p className="text-xs text-muted-foreground">
          kind: {a.kind} · revision {a.revision}
          {a.is_builtin ? " · built-in" : ""}
        </p>
      </button>
      <div className="flex shrink-0 items-center gap-2">
        <Select
          value={a.group_id ?? UNGROUPED}
          onValueChange={(value) =>
            assign.mutate({ id: a.id, groupId: value === UNGROUPED ? null : value })
          }
        >
          <SelectTrigger className="h-8 w-36 text-xs">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={UNGROUPED}>
              {a.is_builtin ? t("common.folder.builtin") : t("common.folder.ungrouped")}
            </SelectItem>
            {sections.folders.map((g) => (
              <SelectItem key={g.id} value={g.id}>
                {g.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <BadgeStatus status={a.status} />
        <Button
          size="icon-sm"
          variant="ghost"
          disabled={a.is_builtin}
          onClick={() => setDeleteTarget(a)}
        >
          <TrashIcon />
        </Button>
      </div>
    </div>
  )

  const renderSection = (key: string, name: string, items: Agent[], group?: AgentGroup) => {
    const openSection = !collapsed.includes(key)
    return (
      <Collapsible
        key={key}
        open={openSection}
        onOpenChange={(next) => toggleSection(key, next)}
        className="grid gap-2"
      >
        <div className="flex items-center gap-1 rounded-md px-1 py-1 hover:bg-accent/40">
          <CollapsibleTrigger asChild>
            <button className="flex min-w-0 flex-1 items-center gap-2 text-left text-sm font-medium">
              <ChevronRightIcon
                className={cn(
                  "size-4 shrink-0 text-muted-foreground transition-transform",
                  openSection && "rotate-90",
                )}
              />
              <FolderIcon className="size-4 shrink-0 text-muted-foreground" />
              <span className="truncate">{name}</span>
              <span className="rounded-full bg-muted px-1.5 text-xs tabular-nums text-muted-foreground">
                {items.length}
              </span>
            </button>
          </CollapsibleTrigger>
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
          {items.length === 0 ? (
            <p className="px-3 py-2 text-xs text-muted-foreground">{t("common.folder.empty")}</p>
          ) : (
            <div className="grid gap-2">{items.map(renderCard)}</div>
          )}
        </CollapsibleContent>
      </Collapsible>
    )
  }

  const hasAny =
    sections.folders.length > 0 || sections.builtin.length > 0 || sections.ungrouped.length > 0

  return (
    <div className="grid gap-4">
      <div className="flex justify-end gap-2">
        <Button variant="outline" onClick={() => setFolderDialog({ mode: "create" })}>
          <FolderPlusIcon /> {t("common.folder.new")}
        </Button>
        <Button onClick={() => setOpen(true)}>
          <PlusIcon /> {t("admin.create")}
        </Button>
      </div>

      {hasAny ? (
        <div className="grid gap-3">
          {sections.folders.map((g) =>
            renderSection(g.id, g.name, sections.byGroup.get(g.id) ?? [], g),
          )}
          {sections.builtin.length > 0 &&
            renderSection(BUILTIN, t("common.folder.builtin"), sections.builtin)}
          {sections.ungrouped.length > 0 &&
            renderSection(UNGROUPED, t("common.folder.ungrouped"), sections.ungrouped)}
        </div>
      ) : (
        <p className="rounded-lg border px-3 py-6 text-center text-sm text-muted-foreground">—</p>
      )}

      {/* Keyed by agent so switching agents remounts the editor: its fields are
          seeded once from props, so without this they'd keep the previous
          agent's text while the tools table reloads. */}
      {selected && <AgentEditor key={selected.id} agent={selected} onSaved={invalidate} />}

      <AgentFormDialog open={open} onOpenChange={setOpen} onCreated={() => { setOpen(false); invalidate() }} />
      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteTarget(null)
        }}
        title={deleteTarget ? t("admin.confirmDelete", { name: deleteTarget.name }) : ""}
        onConfirm={() => {
          if (deleteTarget) remove.mutate(deleteTarget.id)
        }}
      />
      <ConfirmDialog
        open={deleteFolderTarget !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteFolderTarget(null)
        }}
        title={t("common.folder.deleteConfirm")}
        description={t("common.folder.deleteDescription")}
        onConfirm={() => {
          if (deleteFolderTarget) removeGroup.mutate(deleteFolderTarget.id)
        }}
      />
      <GroupNameDialog
        open={folderDialog !== null}
        onOpenChange={(o) => {
          if (!o) setFolderDialog(null)
        }}
        title={
          folderDialog?.mode === "rename" ? t("common.folder.rename") : t("common.folder.new")
        }
        initialName={folderDialog?.mode === "rename" ? folderDialog.group.name : ""}
        confirmLabel={folderDialog?.mode === "rename" ? t("common.save") : t("common.create")}
        pending={createGroup.isPending || renameGroup.isPending}
        onSubmit={(name) => {
          if (!folderDialog) return
          if (folderDialog.mode === "create") createGroup.mutate(name)
          else renameGroup.mutate({ id: folderDialog.group.id, name })
        }}
      />
    </div>
  )
}

function BadgeStatus({ status }: { status: string }) {
  return (
    <span
      className={`rounded-full px-2 py-0.5 text-xs ${
        status === "enabled" ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-900 dark:text-emerald-300" : "bg-muted text-muted-foreground"
      }`}
    >
      {status}
    </span>
  )
}

function AgentEditor({ agent, onSaved }: { agent: Agent; onSaved: () => void }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [name, setName] = useState(agent.name)
  const [systemPrompt, setSystemPrompt] = useState(agent.system_prompt)
  const [description, setDescription] = useState(agent.description)
  const [maxToolCalls, setMaxToolCalls] = useState(agent.max_tool_calls)
  const [enabled, setEnabled] = useState(agent.status === "enabled")

  const assignments = useQuery({
    queryKey: ["admin", "agent-tools", agent.id],
    queryFn: () => adminApi.agents.tools(agent.id),
  })
  const allTools = useQuery({ queryKey: ["admin", "tools"], queryFn: () => adminApi.tools.list() })

  const save = useMutation({
    mutationFn: () =>
      adminApi.agents.update(agent.id, {
        name: name.trim(),
        system_prompt: systemPrompt,
        description,
        max_tool_calls: maxToolCalls,
        status: enabled ? "enabled" : "disabled",
      }),
    onSuccess: () => {
      toast.success("Saved")
      onSaved()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const invalidateAssignments = () =>
    void queryClient.invalidateQueries({ queryKey: ["admin", "agent-tools", agent.id] })

  const assign = useMutation({
    mutationFn: (body: { tool_id: string; requires_approval?: boolean | null; config?: Record<string, unknown> | null }) =>
      adminApi.agents.assignTool(agent.id, body),
    onSuccess: invalidateAssignments,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const unassign = useMutation({
    mutationFn: (toolId: string) => adminApi.agents.unassignTool(agent.id, toolId),
    onSuccess: invalidateAssignments,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const toggleApproval = useMutation({
    mutationFn: ({ toolId, current }: { toolId: string; current: boolean | null }) =>
      adminApi.agents.assignTool(agent.id, {
        tool_id: toolId,
        requires_approval: current === null ? true : !current,
        config: null,
      }),
    onSuccess: invalidateAssignments,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <div className="grid gap-4 rounded-lg border p-4">
      <div className="grid gap-4 md:grid-cols-2">
        <div className="grid gap-2">
          <Label>Name</Label>
          <Input value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label>Description</Label>
          <Input value={description} onChange={(e) => setDescription(e.target.value)} />
        </div>
        <div className="grid gap-2">
          <Label>Max tool calls per run</Label>
          <Input
            type="number"
            min={0}
            value={maxToolCalls}
            onChange={(e) => setMaxToolCalls(Number(e.target.value))}
          />
        </div>
        <div className="grid gap-2 md:col-span-2">
          <Label>System prompt</Label>
          <Textarea rows={5} value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)} />
        </div>
        <label className="flex items-center gap-2 text-sm">
          <Switch checked={enabled} onCheckedChange={setEnabled} /> Enabled
        </label>
        <div className="flex items-end justify-end gap-2">
          <AgentHistoryButton agentId={agent.id} name={agent.name} />
          <Button onClick={() => save.mutate()} disabled={save.isPending || !name.trim()}>
            {t("admin.save")}
          </Button>
        </div>
      </div>

      <div>
        <h3 className="pb-2 text-sm font-semibold">Assigned tools</h3>
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Tool</TableHead>
              <TableHead>Kind</TableHead>
              <TableHead>Approval required</TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {(assignments.data?.tools ?? []).map((a) => (
              <TableRow key={a.tool_id}>
                <TableCell className="font-mono text-sm">{a.name}</TableCell>
                <TableCell>{a.kind}</TableCell>
                <TableCell>
                  <Switch
                    checked={a.requires_approval}
                    onCheckedChange={() => toggleApproval.mutate({ toolId: a.tool_id, current: a.approval_override ? a.requires_approval : null })}
                  />
                </TableCell>
                <TableCell>
                  <Button size="icon-sm" variant="ghost" onClick={() => unassign.mutate(a.tool_id)}>
                    <TrashIcon />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <ToolsSectionPicker
        exclude={(assignments.data?.tools ?? []).map((a) => a.tool_id)}
        allTools={allTools.data?.tools ?? []}
        onPick={(pickedToolId) => assign.mutate({ tool_id: pickedToolId })}
      />
    </div>
  )
}

function AgentFormDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreated: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState("")
  const [slug, setSlug] = useState("")
  const [description, setDescription] = useState("")
  const [systemPrompt, setSystemPrompt] = useState("")

  const create = useMutation({
    mutationFn: () =>
      adminApi.agents.create({
        name,
        slug,
        description,
        system_prompt: systemPrompt,
        max_tool_calls: 200,
      }),
    onSuccess: () => {
      setName("")
      setSlug("")
      setDescription("")
      setSystemPrompt("")
      onCreated()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("admin.agents")} — {t("admin.create")}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label>Slug (lowercase, unique)</Label>
            <Input value={slug} onChange={(e) => setSlug(e.target.value.toLowerCase())} placeholder="my-agent" />
          </div>
          <div className="grid gap-2">
            <Label>Description</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label>System prompt</Label>
            <Textarea rows={4} value={systemPrompt} onChange={(e) => setSystemPrompt(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => create.mutate()} disabled={!name || !slug || create.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
