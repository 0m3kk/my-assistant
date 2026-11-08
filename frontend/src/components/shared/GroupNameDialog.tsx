import { useState } from "react"
import { useTranslation } from "react-i18next"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

/** Create/rename dialog shared by conversation, tool, and agent folders. */
export function GroupNameDialog({
  open,
  onOpenChange,
  title,
  initialName = "",
  confirmLabel,
  pending = false,
  onSubmit,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  initialName?: string
  confirmLabel?: string
  pending?: boolean
  onSubmit: (name: string) => void
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>
        {/* Radix unmounts the content while closed, so the form re-initializes
            from `initialName` every time the dialog opens. */}
        <GroupNameForm
          initialName={initialName}
          confirmLabel={confirmLabel}
          pending={pending}
          onCancel={() => onOpenChange(false)}
          onSubmit={onSubmit}
        />
      </DialogContent>
    </Dialog>
  )
}

function GroupNameForm({
  initialName,
  confirmLabel,
  pending,
  onCancel,
  onSubmit,
}: {
  initialName: string
  confirmLabel?: string
  pending: boolean
  onCancel: () => void
  onSubmit: (name: string) => void
}) {
  const { t } = useTranslation()
  const [name, setName] = useState(initialName)
  const trimmed = name.trim()

  return (
    <>
      <Input
        autoFocus
        value={name}
        maxLength={60}
        placeholder={t("common.folder.name")}
        onChange={(e) => setName(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && trimmed && !pending) onSubmit(trimmed)
        }}
      />
      <DialogFooter>
        <Button variant="outline" onClick={onCancel}>
          {t("confirm.cancel")}
        </Button>
        <Button disabled={!trimmed || pending} onClick={() => onSubmit(trimmed)}>
          {confirmLabel ?? t("common.save")}
        </Button>
      </DialogFooter>
    </>
  )
}
