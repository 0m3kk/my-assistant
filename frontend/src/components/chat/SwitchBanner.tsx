import { ArrowLeftRightIcon } from "lucide-react"
import { useTranslation } from "react-i18next"

export interface SwitchData {
  to?: string
  toName?: string
  status?: string
}

/**
 * Marks the point where general handed the turn to a specialist (and, on the
 * way back, that control returned). The specialist's own output follows
 * normally — it streams into the same message.
 */
export function SwitchBanner({ data }: { data: SwitchData }) {
  const { t } = useTranslation()
  const returned = data.status === "returned"
  const name = data.toName ?? data.to ?? ""
  return (
    <div className="my-1 flex w-fit max-w-full items-center gap-1.5 rounded-full border bg-muted/40 px-2.5 py-1 text-xs text-muted-foreground">
      <ArrowLeftRightIcon className="size-3 shrink-0" />
      <span className="truncate">
        {returned ? t("chat.switch.returned", { name }) : t("chat.switch.to", { name })}
      </span>
    </div>
  )
}
