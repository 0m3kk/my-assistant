import { useMemo, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import {
  BoxIcon,
  ChevronRightIcon,
  FolderIcon,
  FolderPlusIcon,
  GlobeIcon,
  HammerIcon,
  InfoIcon,
  MoreHorizontalIcon,
  PencilIcon,
  PlusIcon,
  SearchIcon,
  TrashIcon,
} from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import type { ToolGroup, ToolRow } from "@/lib/types"
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
import { ConfirmDialog } from "@/components/shared/ConfirmDialog"
import { GroupNameDialog } from "@/components/shared/GroupNameDialog"
import { ToolDetailsDialog } from "./ToolDetailsDialog"
import { WebSearchSettingsDialog } from "./WebSearchSettingsDialog"

const UNGROUPED = "__ungrouped"
const BUILTIN = "__builtin"

type FolderDialogState = { mode: "create" } | { mode: "rename"; group: ToolGroup }

/** Tool catalog + the self-contained tool builder (http / file / wasm). */
export function ToolsSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<ToolRow | null>(null)
  const [details, setDetails] = useState<ToolRow | null>(null)
  const [webSearch, setWebSearch] = useState<ToolRow | null>(null)
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [folderDialog, setFolderDialog] = useState<FolderDialogState | null>(null)
  const [deleteFolderTarget, setDeleteFolderTarget] = useState<ToolGroup | null>(null)

  const tools = useQuery({ queryKey: ["admin", "tools"], queryFn: () => adminApi.tools.list() })
  const groups = useQuery({
    queryKey: ["admin", "tool-groups"],
    queryFn: () => adminApi.toolGroups.list(),
  })

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ["admin", "tools"] })
  const invalidateGroups = () =>
    void queryClient.invalidateQueries({ queryKey: ["admin", "tool-groups"] })

  const remove = useMutation({
    mutationFn: (id: string) => adminApi.tools.remove(id),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const toggle = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) => adminApi.tools.update(id, { status }),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const assign = useMutation({
    mutationFn: ({ id, groupId }: { id: string; groupId: string | null }) =>
      adminApi.tools.update(id, { group_id: groupId }),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const build = useMutation({
    mutationFn: (id: string) => adminApi.tools.build(id),
    onSuccess: () => {
      toast.success("Build finished")
      invalidate()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const createGroup = useMutation({
    mutationFn: (name: string) => adminApi.toolGroups.create(name),
    onSuccess: () => {
      invalidateGroups()
      setFolderDialog(null)
      toast.success(t("common.folder.created"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const renameGroup = useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => adminApi.toolGroups.update(id, name),
    onSuccess: () => {
      invalidateGroups()
      setFolderDialog(null)
      toast.success(t("common.folder.renamed"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const removeGroup = useMutation({
    mutationFn: (id: string) => adminApi.toolGroups.remove(id),
    onSuccess: () => {
      invalidateGroups()
      invalidate()
      setDeleteFolderTarget(null)
      toast.success(t("common.folder.deleted"))
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const sections = useMemo(() => {
    const list = tools.data?.tools ?? []
    const folders = groups.data?.groups ?? []
    const known = new Set(folders.map((g) => g.id))
    const byGroup = new Map<string, ToolRow[]>()
    const builtin: ToolRow[] = []
    const ungrouped: ToolRow[] = []
    for (const tool of list) {
      if (tool.group_id && known.has(tool.group_id)) {
        const bucket = byGroup.get(tool.group_id)
        if (bucket) bucket.push(tool)
        else byGroup.set(tool.group_id, [tool])
      } else if (tool.is_builtin) {
        builtin.push(tool)
      } else {
        ungrouped.push(tool)
      }
    }
    for (const bucket of byGroup.values()) bucket.sort((a, b) => a.name.localeCompare(b.name))
    builtin.sort((a, b) => a.name.localeCompare(b.name))
    ungrouped.sort((a, b) => a.name.localeCompare(b.name))
    return { folders, byGroup, builtin, ungrouped }
  }, [tools.data, groups.data])

  const toggleSection = (key: string, openSection: boolean) => {
    setCollapsed((prev) => {
      if (openSection) return prev.filter((k) => k !== key)
      return prev.includes(key) ? prev : [...prev, key]
    })
  }

  const hasAny =
    sections.folders.length > 0 || sections.builtin.length > 0 || sections.ungrouped.length > 0

  const renderSection = (
    key: string,
    name: string,
    items: ToolRow[],
    group?: ToolGroup,
  ) => {
    const openSection = !collapsed.includes(key)
    return (
      <div key={key} className="rounded-lg border">
        <Collapsible open={openSection} onOpenChange={(next) => toggleSection(key, next)}>
          <div className="flex items-center gap-1 border-b px-2 py-1.5">
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
              <p className="px-3 py-3 text-xs text-muted-foreground">{t("common.folder.empty")}</p>
            ) : (
              <ToolTable
                tools={items}
                groups={sections.folders}
                buildPending={build.isPending}
                onAssign={(id, groupId) => assign.mutate({ id, groupId })}
                onToggle={(tool) =>
                  toggle.mutate({
                    id: tool.id,
                    status: tool.status === "enabled" ? "disabled" : "enabled",
                  })
                }
                onBuild={(id) => build.mutate(id)}
                onDetails={(tool) => setDetails(tool)}
                onWebSearch={(tool) => setWebSearch(tool)}
                onDelete={(tool) => setDeleteTarget(tool)}
              />
            )}
          </CollapsibleContent>
        </Collapsible>
      </div>
    )
  }

  return (
    <div className="grid gap-4">
      <div className="flex justify-end gap-2">
        <Button variant="outline" onClick={() => setFolderDialog({ mode: "create" })}>
          <FolderPlusIcon /> {t("common.folder.new")}
        </Button>
        <Button onClick={() => setOpen(true)}>
          <PlusIcon /> New tool
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

      <ToolFormDialog
        open={open}
        onOpenChange={setOpen}
        onCreated={() => {
          setOpen(false)
          invalidate()
        }}
      />
      <ToolDetailsDialog
        tool={details}
        open={details !== null}
        onOpenChange={(o) => {
          if (!o) setDetails(null)
        }}
      />
      <WebSearchSettingsDialog
        tool={webSearch}
        open={webSearch !== null}
        onOpenChange={(o) => {
          if (!o) setWebSearch(null)
        }}
      />
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

function ToolTable({
  tools,
  groups,
  buildPending,
  onAssign,
  onToggle,
  onBuild,
  onDetails,
  onWebSearch,
  onDelete,
}: {
  tools: ToolRow[]
  groups: ToolGroup[]
  buildPending: boolean
  onAssign: (id: string, groupId: string | null) => void
  onToggle: (tool: ToolRow) => void
  onBuild: (id: string) => void
  onDetails: (tool: ToolRow) => void
  onWebSearch: (tool: ToolRow) => void
  onDelete: (tool: ToolRow) => void
}) {
  const { t } = useTranslation()
  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Name</TableHead>
          <TableHead>Kind</TableHead>
          <TableHead>Description</TableHead>
          <TableHead>Folder</TableHead>
          <TableHead>Approval</TableHead>
          <TableHead>Status</TableHead>
          <TableHead className="w-40" />
        </TableRow>
      </TableHeader>
      <TableBody>
        {tools.map((tool) => (
          <TableRow key={tool.id}>
            <TableCell className="font-mono text-sm">{tool.name}</TableCell>
            <TableCell>
              <span className="flex items-center gap-1">
                {tool.kind === "http" && <GlobeIcon className="size-3.5 text-muted-foreground" />}
                {tool.kind === "wasm" && <BoxIcon className="size-3.5 text-muted-foreground" />}
                {tool.kind}
              </span>
            </TableCell>
            <TableCell className="max-w-72 truncate text-muted-foreground">{tool.description}</TableCell>
            <TableCell>
              <Select
                value={tool.group_id ?? UNGROUPED}
                onValueChange={(value) => onAssign(tool.id, value === UNGROUPED ? null : value)}
              >
                <SelectTrigger className="h-8 w-40 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={UNGROUPED}>
                    {tool.is_builtin
                      ? t("common.folder.builtin")
                      : t("common.folder.ungrouped")}
                  </SelectItem>
                  {groups.map((g) => (
                    <SelectItem key={g.id} value={g.id}>
                      {g.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </TableCell>
            <TableCell>{tool.requires_approval ? "yes" : "—"}</TableCell>
            <TableCell>
              <Switch
                checked={tool.status === "enabled"}
                onCheckedChange={() => onToggle(tool)}
              />
            </TableCell>
            <TableCell>
              <div className="flex items-center justify-end gap-1">
                {tool.name === "web_search" && (
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    onClick={() => onWebSearch(tool)}
                    title="Web search settings"
                  >
                    <SearchIcon />
                  </Button>
                )}
                {tool.kind === "wasm" && (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={buildPending}
                    onClick={() => onBuild(tool.id)}
                    title="Build the wasm artifact"
                  >
                    <HammerIcon /> Build
                  </Button>
                )}
                <Button
                  size="icon-sm"
                  variant="ghost"
                  onClick={() => onDetails(tool)}
                  title="Build, test, and version history"
                >
                  <InfoIcon />
                </Button>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  disabled={tool.is_builtin}
                  onClick={() => onDelete(tool)}
                >
                  <TrashIcon />
                </Button>
              </div>
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

/** Picker used inside the Agents editor to assign tools. */
export function ToolsSectionPicker({
  exclude,
  allTools,
  onPick,
}: {
  exclude: string[]
  allTools: ToolRow[]
  onPick: (toolId: string) => void
}) {
  const available = allTools.filter((tool) => !exclude.includes(tool.id))
  if (available.length === 0) return null
  return (
    <div>
      <h3 className="pb-2 text-sm font-semibold">Assign a tool</h3>
      <div className="flex flex-wrap gap-2">
        {available.map((tool) => (
          <Button key={tool.id} size="sm" variant="outline" onClick={() => onPick(tool.id)}>
            <PlusIcon /> {tool.name}
          </Button>
        ))}
      </div>
    </div>
  )
}

const DEFAULT_SCHEMA = '{\n  "type": "object",\n  "properties": {}\n}'

function ToolFormDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreated: () => void
}) {
  const { t } = useTranslation()
  const [kind, setKind] = useState<"http" | "file" | "wasm">("http")
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [schema, setSchema] = useState(DEFAULT_SCHEMA)
  const [requiresApproval, setRequiresApproval] = useState(false)

  // HTTP
  const [method, setMethod] = useState("GET")
  const [baseUrl, setBaseUrl] = useState("")
  const [pathTemplate, setPathTemplate] = useState("")
  const [authHeader, setAuthHeader] = useState("")
  const [queryJson, setQueryJson] = useState("")
  const [bodyTemplate, setBodyTemplate] = useState("")
  const [responseMapping, setResponseMapping] = useState("")

  // File
  const [operation, setOperation] = useState("read")
  const [root, setRoot] = useState("@workspace")
  const [allowGlobs, setAllowGlobs] = useState("")
  const [denyGlobs, setDenyGlobs] = useState("")
  const [maxFileBytes, setMaxFileBytes] = useState(1048576)

  // Wasm
  const [language, setLanguage] = useState("rust")
  const [source, setSource] = useState("")
  const [capabilities, setCapabilities] = useState("{}")

  const fileWrite = kind === "file" && (operation === "write" || operation === "edit")

  const create = useMutation({
    mutationFn: () => {
      let parametersSchema: Record<string, unknown>
      try {
        parametersSchema = JSON.parse(schema)
      } catch {
        throw new Error("parameters schema is not valid JSON")
      }
      let config: Record<string, unknown>
      if (kind === "http") {
        config = {
          base_url: baseUrl,
          method,
          path_template: pathTemplate,
          timeout_ms: 20000,
          max_output_bytes: 262144,
        }
        const headers: Record<string, string> = {}
        if (authHeader.trim()) headers.Authorization = authHeader.trim()
        if (Object.keys(headers).length > 0) config.headers = headers
        if (queryJson.trim()) {
          try {
            config.query = JSON.parse(queryJson)
          } catch {
            throw new Error("query is not valid JSON")
          }
        }
        if (bodyTemplate.trim()) config.body_template = bodyTemplate
        if (responseMapping.trim()) config.response_map = responseMapping
      } else if (kind === "file") {
        config = {
          operation,
          root,
          max_file_bytes: Number(maxFileBytes) || 1048576,
        }
        const allow = allowGlobs.split(",").map((s) => s.trim()).filter(Boolean)
        const deny = denyGlobs.split(",").map((s) => s.trim()).filter(Boolean)
        if (allow.length) config.allow_globs = allow
        if (deny.length) config.deny_globs = deny
      } else {
        config = {
          runtime: "wasm",
          language,
          source,
          timeout_ms: 10000,
          memory_limit_mb: 64,
        }
        try {
          config.capabilities = JSON.parse(capabilities || "{}")
        } catch {
          throw new Error("capabilities is not valid JSON")
        }
      }
      return adminApi.tools.create({
        name,
        display_name: name,
        description,
        kind,
        config,
        parameters_schema: parametersSchema,
        requires_approval: requiresApproval || fileWrite,
      })
    },
    onSuccess: () => {
      toast.success("Tool created")
      setName("")
      setDescription("")
      setBaseUrl("")
      setPathTemplate("")
      setSource("")
      onCreated()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e)),
  })

  const disabled =
    !name ||
    (kind === "http" && (!baseUrl || !pathTemplate)) ||
    (kind === "file" && !root) ||
    (kind === "wasm" && !source) ||
    create.isPending

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[88dvh] overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Tool builder</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid grid-cols-3 gap-2">
            <div className="grid gap-2">
              <Label>Kind</Label>
              <Select value={kind} onValueChange={(v) => setKind(v as typeof kind)}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="http">http (one endpoint)</SelectItem>
                  <SelectItem value="file">file (scoped root)</SelectItem>
                  <SelectItem value="wasm">wasm (code)</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="col-span-2 grid gap-2">
              <Label>Tool name (lowercase.dotted ok)</Label>
              <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="github.get_pr_diff" />
            </div>
          </div>
          <div className="grid gap-2">
            <Label>Description (the LLM reads this)</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>

          {kind === "http" && (
            <>
              <div className="grid grid-cols-4 gap-2">
                <div className="col-span-3 grid gap-2">
                  <Label>Base URL (fixed host)</Label>
                  <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.github.com" />
                </div>
                <div className="grid gap-2">
                  <Label>Method</Label>
                  <Select value={method} onValueChange={setMethod}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {["GET", "POST", "PUT", "PATCH", "DELETE"].map((m) => (
                        <SelectItem key={m} value={m}>
                          {m}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              </div>
              <div className="grid gap-2">
                <Label>Path template — {"{param}"} placeholders come from the schema</Label>
                <Input
                  value={pathTemplate}
                  onChange={(e) => setPathTemplate(e.target.value)}
                  placeholder="/repos/{owner}/{repo}/pulls/{number}"
                />
              </div>
              <div className="grid gap-2">
                <Label>Authorization header — use {"{{secret:NAME}}"}</Label>
                <Input
                  value={authHeader}
                  onChange={(e) => setAuthHeader(e.target.value)}
                  placeholder="Bearer {{secret:github_token}}"
                />
              </div>
              <div className="grid gap-2">
                <Label>Query JSON (optional)</Label>
                <Input value={queryJson} onChange={(e) => setQueryJson(e.target.value)} placeholder='{"per_page": "100"}' />
              </div>
              {method !== "GET" && (
                <div className="grid gap-2">
                  <Label>Body template (JSON, {"{param}"} placeholders)</Label>
                  <Textarea rows={3} value={bodyTemplate} onChange={(e) => setBodyTemplate(e.target.value)} />
                </div>
              )}
              <div className="grid gap-2">
                <Label>Response map (dot path, optional)</Label>
                <Input value={responseMapping} onChange={(e) => setResponseMapping(e.target.value)} placeholder="data.items" />
              </div>
            </>
          )}

          {kind === "file" && (
            <>
              <div className="grid grid-cols-2 gap-2">
                <div className="grid gap-2">
                  <Label>Operation</Label>
                  <Select value={operation} onValueChange={setOperation}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {["read", "list", "search", "write", "edit"].map((op) => (
                        <SelectItem key={op} value={op}>
                          {op}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
                <div className="grid gap-2">
                  <Label>Root (@workspace or absolute)</Label>
                  <Input value={root} onChange={(e) => setRoot(e.target.value)} placeholder="/Users/me/projects/foo" />
                </div>
              </div>
              <div className="grid gap-2">
                <Label>Allow globs (comma separated, optional)</Label>
                <Input value={allowGlobs} onChange={(e) => setAllowGlobs(e.target.value)} placeholder="src/**, tests/**" />
              </div>
              <div className="grid gap-2">
                <Label>Deny globs (comma separated, optional)</Label>
                <Input value={denyGlobs} onChange={(e) => setDenyGlobs(e.target.value)} placeholder=".git/**, .env*" />
              </div>
              <div className="grid gap-2">
                <Label>Max file bytes</Label>
                <Input
                  type="number"
                  value={maxFileBytes}
                  onChange={(e) => setMaxFileBytes(Number(e.target.value))}
                />
              </div>
            </>
          )}

          {kind === "wasm" && (
            <>
              <div className="grid grid-cols-2 gap-2">
                <div className="grid gap-2">
                  <Label>Language</Label>
                  <Select value={language} onValueChange={setLanguage}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="rust">rust</SelectItem>
                      <SelectItem value="assemblyscript">assemblyscript</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
                <div className="grid gap-2">
                  <Label>Capabilities JSON</Label>
                  <Input value={capabilities} onChange={(e) => setCapabilities(e.target.value)} placeholder='{"fs":{"roots":["@workspace"]}}' />
                </div>
              </div>
              <div className="grid gap-2">
                <Label>Source</Label>
                <Textarea
                  rows={12}
                  className="font-mono text-xs"
                  value={source}
                  onChange={(e) => setSource(e.target.value)}
                  placeholder="// export alloc(len) and run(ptr,len)..."
                />
              </div>
              <p className="text-xs text-muted-foreground">
                The tool stays disabled until an admin triggers Build. The builder runs in a
                sandboxed container; this action is never exposed to agents. See docs/TOOL_ABI.md
                for the ABI (alloc/run + host imports).
              </p>
            </>
          )}

          <div className="grid gap-2">
            <Label>Parameters JSON Schema</Label>
            <Textarea rows={6} className="font-mono text-xs" value={schema} onChange={(e) => setSchema(e.target.value)} />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Switch
              checked={requiresApproval || fileWrite}
              onCheckedChange={setRequiresApproval}
              disabled={fileWrite}
            />
            Require approval before runs
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => create.mutate()} disabled={disabled}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
