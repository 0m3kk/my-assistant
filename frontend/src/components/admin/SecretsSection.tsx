import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { toast } from "sonner"
import { KeyRoundIcon, PlusIcon, TrashIcon } from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import type { SecretPublic } from "@/lib/types"
import { ApiError } from "@/lib/api/client"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
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
import { ConfirmDialog } from "@/components/shared/ConfirmDialog"

export function SecretsSection() {
  const { t } = useTranslation()
  const queryClient = useQueryClient()
  const [creating, setCreating] = useState(false)
  const [rotating, setRotating] = useState<SecretPublic | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<SecretPublic | null>(null)

  const secrets = useQuery({ queryKey: ["admin", "secrets"], queryFn: () => adminApi.secrets.list() })

  const invalidate = () => void queryClient.invalidateQueries({ queryKey: ["admin", "secrets"] })

  const remove = useMutation({
    mutationFn: (id: string) => adminApi.secrets.remove(id),
    onSuccess: () => {
      toast.success("Deleted")
      invalidate()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <div className="grid gap-4">
      <p className="text-sm text-muted-foreground">
        Secrets are encrypted at rest (AES-256-GCM) and referenced from tools/providers as{" "}
        <code className="rounded bg-muted px-1 py-0.5 text-xs">{"{{secret:NAME}}"}</code>. Values are injected only
        at execution time — they never reach the model, logs, or API responses.
      </p>

      <div className="flex justify-end">
        <Button onClick={() => setCreating(true)}>
          <PlusIcon /> {t("admin.create")}
        </Button>
      </div>

      <div className="rounded-lg border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Description</TableHead>
              <TableHead>Value</TableHead>
              <TableHead>Version</TableHead>
              <TableHead>Last used</TableHead>
              <TableHead className="w-10" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {(secrets.data?.secrets ?? []).map((s) => (
              <TableRow key={s.id} className="cursor-pointer" onClick={() => setRotating(s)}>
                <TableCell className="font-mono text-sm">
                  <span className="flex items-center gap-1.5">
                    <KeyRoundIcon className="size-3.5 text-muted-foreground" />
                    {s.name}
                  </span>
                </TableCell>
                <TableCell className="text-muted-foreground">{s.description}</TableCell>
                <TableCell className="font-mono text-xs">{s.masked}</TableCell>
                <TableCell>v{s.version}</TableCell>
                <TableCell className="text-muted-foreground">
                  {s.last_used_at ? new Date(s.last_used_at).toLocaleString() : "—"}
                </TableCell>
                <TableCell>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    onClick={(e) => {
                      e.stopPropagation()
                      setDeleteTarget(s)
                    }}
                  >
                    <TrashIcon />
                  </Button>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      <SecretFormDialog open={creating} onOpenChange={setCreating} onSaved={() => { setCreating(false); invalidate() }} />
      <SecretRotateDialog secret={rotating} onOpenChange={(o) => !o && setRotating(null)} onSaved={() => { setRotating(null); invalidate() }} />
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

function SecretFormDialog({
  open,
  onOpenChange,
  onSaved,
}: {
  open: boolean
  onOpenChange: (o: boolean) => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState("")
  const [description, setDescription] = useState("")
  const [value, setValue] = useState("")

  const create = useMutation({
    mutationFn: () => adminApi.secrets.create({ name, description, value }),
    onSuccess: () => {
      toast.success("Secret created")
      setName("")
      setDescription("")
      setValue("")
      onSaved()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("admin.secrets")} — {t("admin.create")}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>Name</Label>
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="weather_api_key" />
          </div>
          <div className="grid gap-2">
            <Label>Description</Label>
            <Input value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label>Value (stored encrypted, never displayed again)</Label>
            <Input type="password" value={value} onChange={(e) => setValue(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => create.mutate()} disabled={!name || !value || create.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function SecretRotateDialog({
  secret,
  onOpenChange,
  onSaved,
}: {
  secret: SecretPublic | null
  onOpenChange: (o: boolean) => void
  onSaved: () => void
}) {
  const { t } = useTranslation()
  const [value, setValue] = useState("")

  const update = useMutation({
    mutationFn: () => adminApi.secrets.update(secret!.id, { value }),
    onSuccess: () => {
      toast.success("Secret rotated")
      setValue("")
      onSaved()
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={!!secret} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rotate — {secret?.name}</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>New value (blank = keep current)</Label>
            <Input type="password" value={value} onChange={(e) => setValue(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            {t("admin.cancel")}
          </Button>
          <Button onClick={() => update.mutate()} disabled={!value || update.isPending}>
            {t("admin.save")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
