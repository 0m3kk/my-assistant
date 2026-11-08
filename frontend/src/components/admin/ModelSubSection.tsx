import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { PencilIcon, PlusIcon, RefreshCwIcon, TrashIcon } from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import { REASONING_EFFORT_LEVELS, type Model, type ReasoningEffort } from "@/lib/types"
import { ApiError } from "@/lib/api/client"
import { formatContextWindow } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import { Badge } from "@/components/ui/badge"
import { Spinner } from "@/components/shared/Spinner"
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

export function ModelSubSection({ providerId }: { providerId: string }) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editTarget, setEditTarget] = useState<Model | null>(null)

  const models = useQuery({
    queryKey: ["admin", "models", providerId],
    queryFn: () => adminApi.models.list(providerId),
  })

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["admin", "models", providerId] })
    void queryClient.invalidateQueries({ queryKey: ["models"] })
  }

  const remove = useMutation({
    mutationFn: (id: string) => adminApi.models.remove(id),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const setDefault = useMutation({
    mutationFn: ({ id, isDefault }: { id: string; isDefault: boolean }) =>
      adminApi.models.update(id, { is_default: isDefault }),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <div className="rounded-lg border p-4">
      <div className="flex items-center justify-between pb-2">
        <h3 className="text-sm font-semibold">{t("admin.models")}</h3>
        <div className="flex gap-2">
          <DiscoverModelsDialog providerId={providerId} onImported={invalidate} />
          <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
            <PlusIcon /> {t("admin.create")}
          </Button>
        </div>
      </div>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Model ID</TableHead>
            <TableHead>Context</TableHead>
            <TableHead>Capabilities</TableHead>
            <TableHead>{t("admin.reasoningEffort")}</TableHead>
            <TableHead>Default</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {(models.data?.models ?? []).map((m) => (
            <ModelRow
              key={m.id}
              model={m}
              onEdit={() => setEditTarget(m)}
              onRemove={() => remove.mutate(m.id)}
              onSetDefault={(v) => setDefault.mutate({ id: m.id, isDefault: v })}
            />
          ))}
        </TableBody>
      </Table>

      <ModelFormDialog providerId={providerId} open={open} onOpenChange={setOpen} onCreated={invalidate} />
      {editTarget && (
        <ModelEditDialog
          key={editTarget.id}
          model={editTarget}
          onOpenChange={(o) => {
            if (!o) setEditTarget(null)
          }}
          onSaved={invalidate}
        />
      )}
    </div>
  )
}

function ModelRow({
  model,
  onEdit,
  onRemove,
  onSetDefault,
}: {
  model: Model
  onEdit: () => void
  onRemove: () => void
  onSetDefault: (v: boolean) => void
}) {
  const { t } = useTranslation()
  const queryClient = useQueryClient()

  const update = useMutation({
    mutationFn: (body: { capabilities?: Record<string, boolean>; reasoning_effort?: ReasoningEffort | null }) =>
      adminApi.models.update(model.id, body),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["admin", "models", model.provider_id] }),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <TableRow>
      <TableCell>{model.display_name}</TableCell>
      <TableCell className="font-mono text-sm">{model.api_name}</TableCell>
      <TableCell className="text-sm text-muted-foreground">
        {model.context_window ? formatContextWindow(model.context_window) : "—"}
      </TableCell>
      <TableCell className="flex gap-1">
        {(["tools", "vision", "reasoning"] as const).map((cap) => (
          <button
            key={cap}
            onClick={() =>
              update.mutate({ capabilities: { ...model.capabilities, [cap]: !model.capabilities[cap] } })
            }
            title={`Toggle ${cap}`}
          >
            <Badge variant={model.capabilities[cap] ? "default" : "outline"}>{cap}</Badge>
          </button>
        ))}
      </TableCell>
      <TableCell>
        {/* "default" sentinel = provider default (no reasoning_effort sent upstream). */}
        <Select
          value={model.reasoning_effort ?? "default"}
          onValueChange={(v) => update.mutate({ reasoning_effort: v === "default" ? null : (v as ReasoningEffort) })}
        >
          <SelectTrigger size="sm" className="w-28" aria-label={t("admin.reasoningEffort")}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="default">Default</SelectItem>
            {REASONING_EFFORT_LEVELS.map((e) => (
              <SelectItem key={e} value={e}>
                {t(`chat.effort.${e}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </TableCell>
      <TableCell>
        <Switch checked={model.is_default} onCheckedChange={onSetDefault} />
      </TableCell>
      <TableCell>
        <Button size="icon-sm" variant="ghost" onClick={onEdit} aria-label={t("admin.edit")}>
          <PencilIcon />
        </Button>
        <Button size="icon-sm" variant="ghost" onClick={onRemove} aria-label={t("admin.delete")}>
          <TrashIcon />
        </Button>
      </TableCell>
    </TableRow>
  )
}

function DiscoverModelsDialog({
  providerId,
  onImported,
}: {
  providerId: string
  onImported: () => void
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [selected, setSelected] = useState<Set<string>>(new Set())

  // Fetched when the dialog opens; refetched on an explicit refresh.
  const discovered = useQuery({
    queryKey: ["admin", "models", providerId, "discover"],
    queryFn: () => adminApi.models.discover(providerId),
    enabled: open,
    retry: false,
    staleTime: 0,
  })

  const candidates = (discovered.data?.models ?? []).filter((m) => !m.configured)

  const importModels = useMutation({
    mutationFn: (names: string[]) => adminApi.models.importDiscovered(providerId, names),
    onSuccess: (result) => {
      toast.success(
        `Added ${result.created_count} model(s)` +
          (result.skipped_count ? `, skipped ${result.skipped_count}` : ""),
      )
      setSelected(new Set())
      setOpen(false)
      onImported()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const toggle = (apiName: string) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(apiName)) next.delete(apiName)
      else next.add(apiName)
      return next
    })
  }

  return (
    <>
      <Button
        size="sm"
        variant="outline"
        onClick={() => setOpen(true)}
        title={t("admin.discover.title")}
      >
        <RefreshCwIcon /> {t("admin.discover.button")}
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>{t("admin.discover.title")}</DialogTitle>
          </DialogHeader>

          {discovered.isPending && (
            <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
              <Spinner className="size-4" /> {t("admin.discover.loading")}
            </div>
          )}

          {discovered.isError && (
            <p className="py-4 text-sm text-destructive">
              {t("admin.discover.failed")}:{" "}
              {discovered.error instanceof ApiError
                ? discovered.error.message
                : String(discovered.error)}
            </p>
          )}

          {discovered.isSuccess && candidates.length === 0 && (
            <p className="py-4 text-sm text-muted-foreground">
              {discovered.data.models.length === 0
                ? t("admin.discover.empty")
                : t("admin.discover.allConfigured")}
            </p>
          )}

          {candidates.length > 0 && (
            <div className="max-h-80 overflow-y-auto rounded-md border">
              {candidates.map((m) => (
                <label
                  key={m.api_name}
                  className="flex cursor-pointer items-center gap-3 border-b px-3 py-2 text-sm last:border-b-0 hover:bg-muted/40"
                >
                  <input
                    type="checkbox"
                    className="size-4"
                    checked={selected.has(m.api_name)}
                    onChange={() => toggle(m.api_name)}
                  />
                  <span className="flex min-w-0 flex-col">
                    <span className="truncate font-mono text-xs">{m.api_name}</span>
                    {m.display_name !== m.api_name && (
                      <span className="truncate text-xs text-muted-foreground">
                        {m.display_name}
                      </span>
                    )}
                  </span>
                  {m.context_window && (
                    <Badge variant="outline" className="ml-auto shrink-0">
                      {formatContextWindow(m.context_window)}
                    </Badge>
                  )}
                </label>
              ))}
            </div>
          )}

          <DialogFooter>
            <Button variant="outline" onClick={() => setOpen(false)}>
              {t("admin.cancel")}
            </Button>
            <Button
              onClick={() => importModels.mutate([...selected])}
              disabled={selected.size === 0 || importModels.isPending}
            >
              {t("admin.discover.addSelected", { count: selected.size })}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

function ModelFormDialog({
  providerId,
  open,
  onOpenChange,
  onCreated,
}: {
  providerId: string
  open: boolean
  onOpenChange: (o: boolean) => void
  onCreated: () => void
}) {
  const { t } = useTranslation()
  const [apiName, setApiName] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [contextWindow, setContextWindow] = useState("")
  const [tools, setTools] = useState(true)
  const [effort, setEffort] = useState("default")

  const create = useMutation({
    mutationFn: () =>
      adminApi.models.create({
        provider_id: providerId,
        api_name: apiName,
        display_name: displayName || undefined,
        context_window: contextWindow ? Number(contextWindow) : null,
        capabilities: { tools },
        reasoning_effort: effort === "default" ? undefined : (effort as ReasoningEffort),
      }),
    onSuccess: () => {
      toast.success("Model created")
      setApiName("")
      setDisplayName("")
      setContextWindow("")
      setEffort("default")
      onOpenChange(false)
      onCreated()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("admin.models")} — {t("admin.create")}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Name</Label>
            <Input value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
            <p className="text-xs text-muted-foreground">
              Shown in the chat model selector. Defaults to the model ID when empty.
            </p>
          </div>
          <div className="grid gap-2">
            <Label>Model ID</Label>
            <Input value={apiName} onChange={(e) => setApiName(e.target.value)} placeholder="gpt-4o, glm-4.6, …" />
            <p className="text-xs text-muted-foreground">
              Exact identifier the provider expects. Not the label shown in chat.
            </p>
          </div>
          <div className="grid gap-2">
            <Label>Context window (tokens, optional)</Label>
            <Input
              type="number"
              min={1}
              value={contextWindow}
              onChange={(e) => setContextWindow(e.target.value)}
              placeholder="128000"
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("admin.reasoningEffort")}</Label>
            <Select value={effort} onValueChange={setEffort}>
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("chat.effort.default")}</SelectItem>
                {REASONING_EFFORT_LEVELS.map((e) => (
                  <SelectItem key={e} value={e}>
                    {t(`chat.effort.${e}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Switch checked={tools} onCheckedChange={setTools} /> Supports tool calling
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => create.mutate()} disabled={!apiName || create.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ModelEditDialog({
  model,
  onOpenChange,
  onSaved,
}: {
  model: Model
  onOpenChange: (o: boolean) => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [apiName, setApiName] = useState(model.api_name)
  const [displayName, setDisplayName] = useState(model.display_name)
  const [contextWindow, setContextWindow] = useState(model.context_window?.toString() ?? "")
  const [effort, setEffort] = useState(model.reasoning_effort ?? "default")
  const [tools, setTools] = useState(!!model.capabilities.tools)
  const [vision, setVision] = useState(!!model.capabilities.vision)
  const [reasoning, setReasoning] = useState(!!model.capabilities.reasoning)
  const [isDefault, setIsDefault] = useState(model.is_default)
  const [status, setStatus] = useState(model.status)

  const update = useMutation({
    mutationFn: () =>
      adminApi.models.update(model.id, {
        api_name: apiName.trim(),
        display_name: displayName.trim() || apiName.trim(),
        context_window: contextWindow ? Number(contextWindow) : null,
        capabilities: { tools, vision, reasoning },
        reasoning_effort: effort === "default" ? null : (effort as ReasoningEffort),
        is_default: isDefault,
        status,
      }),
    onSuccess: () => {
      toast.success("Model updated")
      onOpenChange(false)
      onSaved()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {t("admin.models")} — {t("admin.edit")}
          </DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Name</Label>
            <Input
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              placeholder={apiName}
            />
            <p className="text-xs text-muted-foreground">
              Shown in the chat model selector. Defaults to the model ID when empty.
            </p>
          </div>
          <div className="grid gap-2">
            <Label>Model ID</Label>
            <Input
              value={apiName}
              onChange={(e) => {
                const next = e.target.value
                // Names usually mirror the model ID until customized. Keep them
                // in sync so renaming here also updates the model selector.
                if (!displayName || displayName === apiName) setDisplayName(next)
                setApiName(next)
              }}
            />
            <p className="text-xs text-muted-foreground">
              Exact identifier the provider expects. Not the label shown in chat.
            </p>
          </div>
          <div className="grid gap-2">
            <Label>Context window (tokens, optional)</Label>
            <Input
              type="number"
              min={1}
              value={contextWindow}
              onChange={(e) => setContextWindow(e.target.value)}
              placeholder="128000"
            />
          </div>
          <div className="grid gap-2">
            <Label>{t("admin.reasoningEffort")}</Label>
            <Select value={effort} onValueChange={setEffort}>
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("chat.effort.default")}</SelectItem>
                {REASONING_EFFORT_LEVELS.map((e) => (
                  <SelectItem key={e} value={e}>
                    {t(`chat.effort.${e}`)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid gap-2">
            <Label>Capabilities</Label>
            <div className="flex flex-wrap gap-4">
              <label className="flex items-center gap-2 text-sm">
                <Switch checked={tools} onCheckedChange={setTools} /> Tool calling
              </label>
              <label className="flex items-center gap-2 text-sm">
                <Switch checked={vision} onCheckedChange={setVision} /> Vision
              </label>
              <label className="flex items-center gap-2 text-sm">
                <Switch checked={reasoning} onCheckedChange={setReasoning} /> Reasoning
              </label>
            </div>
          </div>
          <div className="grid gap-2">
            <Label>Status</Label>
            <Select value={status} onValueChange={setStatus}>
              <SelectTrigger className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="enabled">Enabled</SelectItem>
                <SelectItem value="disabled">Disabled</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <label className="flex items-center gap-2 text-sm">
            <Switch checked={isDefault} onCheckedChange={setIsDefault} /> Default model
          </label>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => update.mutate()} disabled={!apiName.trim() || update.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
