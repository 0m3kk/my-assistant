import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { ChevronDownIcon, ChevronRightIcon, PencilIcon, PlusIcon, TrashIcon } from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import type { Provider } from "@/lib/types"
import { ApiError } from "@/lib/api/client"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Switch } from "@/components/ui/switch"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { ModelSubSection } from "./ModelSubSection"
import { ConfirmDialog } from "@/components/shared/ConfirmDialog"

export function ProvidersSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Provider | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<Provider | null>(null)

  const providers = useQuery({ queryKey: ["admin", "providers"], queryFn: () => adminApi.providers.list() })
  const presets = useQuery({ queryKey: ["admin", "presets"], queryFn: () => adminApi.providerPresets() })

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ["admin", "providers"] })

  const remove = useMutation({
    mutationFn: (id: string) => adminApi.providers.remove(id),
    onSuccess: () => {
      toast.success("Deleted")
      invalidate()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const toggle = useMutation({
    mutationFn: ({ id, status }: { id: string; status: string }) =>
      adminApi.providers.update(id, { status }),
    onSuccess: invalidate,
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <div className="grid gap-4">
      <div className="flex justify-end">
        <Button onClick={() => setOpen(true)}>
          <PlusIcon /> {t("admin.create")}
        </Button>
      </div>

      <div className="rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-8" />
              <TableHead>Name</TableHead>
              <TableHead>Preset</TableHead>
              <TableHead>Base URL</TableHead>
              <TableHead>API key</TableHead>
              <TableHead>Status</TableHead>
              <TableHead className="w-20" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {(providers.data?.providers ?? []).map((p) => (
              <TableRow
                key={p.id}
                className="cursor-pointer"
                onClick={() => setExpandedId(expandedId === p.id ? null : p.id)}
              >
                <TableCell>
                  {expandedId === p.id ? (
                    <ChevronDownIcon className="size-4 text-muted-foreground" />
                  ) : (
                    <ChevronRightIcon className="size-4 text-muted-foreground" />
                  )}
                </TableCell>
                <TableCell className="font-medium">{p.name}</TableCell>
                <TableCell>{p.preset}</TableCell>
                <TableCell className="max-w-64 truncate text-muted-foreground">{p.base_url}</TableCell>
                <TableCell>{p.secret_id ? "••••" : "—"}</TableCell>
                <TableCell>
                  <Switch
                    checked={p.status === "enabled"}
                    onClick={(e) => {
                      e.stopPropagation()
                      toggle.mutate({ id: p.id, status: p.status === "enabled" ? "disabled" : "enabled" })
                    }}
                  />
                </TableCell>
                <TableCell>
                  <div className="flex gap-1">
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      aria-label={t("admin.edit")}
                      onClick={(e) => {
                        e.stopPropagation()
                        setEditing(p)
                      }}
                    >
                      <PencilIcon />
                    </Button>
                    <Button
                      size="icon-sm"
                      variant="ghost"
                      onClick={(e) => {
                        e.stopPropagation()
                        setDeleteTarget(p)
                      }}
                    >
                      <TrashIcon />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
            {expandedId && providers.data?.providers
              .filter((p) => p.id === expandedId)
              .map((p) => (
                <TableRow key={`${p.id}-models`}>
                  <TableCell colSpan={7} className="bg-muted/30 p-4">
                    <ModelSubSection providerId={p.id} />
                  </TableCell>
                </TableRow>
              ))}
          </TableBody>
        </Table>
      </div>

      <ProviderFormDialog
        open={open}
        onOpenChange={setOpen}
        presets={presets.data ?? []}
        onCreated={() => {
          setOpen(false)
          invalidate()
        }}
      />
      <ProviderEditDialog
        provider={editing}
        onOpenChange={(o) => !o && setEditing(null)}
        onSaved={() => {
          setEditing(null)
          invalidate()
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
    </div>
  )
}

function ProviderFormDialog({
  open,
  onOpenChange,
  presets,
  onCreated,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  presets: { id: string; label: string; base_url: string; hint: string }[]
  onCreated: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState("")
  const [preset, setPreset] = useState("custom")
  const [baseUrl, setBaseUrl] = useState("")
  const [apiKey, setApiKey] = useState("")

  const create = useMutation({
    mutationFn: () =>
      adminApi.providers.create({
        name,
        preset,
        base_url: baseUrl,
        api_key: apiKey || undefined,
      }),
    onSuccess: () => {
      toast.success("Provider created")
      setName("")
      setBaseUrl("")
      setApiKey("")
      onCreated()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const selectedPreset = presets.find((p) => p.id === preset)

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("admin.providers")} — {t("admin.create")}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Preset</Label>
            <Select
              value={preset}
              onValueChange={(v) => {
                setPreset(v)
                const p = presets.find((x) => x.id === v)
                if (p?.base_url) setBaseUrl(p.base_url)
              }}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {presets.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {selectedPreset && <p className="text-xs text-muted-foreground">{selectedPreset.hint}</p>}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="provider-name">Name</Label>
            <Input id="provider-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="provider-url">Base URL</Label>
            <Input
              id="provider-url"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="https://…/v1"
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="provider-key">API key (optional — Ollama needs none)</Label>
            <Input
              id="provider-key"
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-…"
            />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => create.mutate()} disabled={!name || !baseUrl || create.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ProviderEditDialog({
  provider,
  onOpenChange,
  onSaved,
}: {
  provider: Provider | null
  onOpenChange: (o: boolean) => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState(provider?.name ?? "")
  const [baseUrl, setBaseUrl] = useState(provider?.base_url ?? "")
  const [apiKey, setApiKey] = useState("")

  // Sync fields when a different provider is selected.
  const [lastId, setLastId] = useState(provider?.id)
  if (provider && provider.id !== lastId) {
    setLastId(provider.id)
    setName(provider.name)
    setBaseUrl(provider.base_url)
    setApiKey("")
  }

  const save = useMutation({
    mutationFn: () =>
      adminApi.providers.update(provider!.id, {
        name,
        base_url: baseUrl,
        api_key: apiKey || undefined,
      }),
    onSuccess: () => {
      toast.success("Saved")
      onSaved()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={!!provider} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("admin.edit")} — {provider?.name}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label>Base URL</Label>
            <Input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label>Rotate API key (leave blank to keep)</Label>
            <Input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
