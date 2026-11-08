import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import { CheckIcon, CopyIcon } from "lucide-react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

interface CopyButtonProps {
  /** Plain text written to the clipboard. */
  value: string
  className?: string
}

/** Copies `value` to the clipboard, flashing a check mark for two seconds. */
export function CopyButton({ value, className }: CopyButtonProps) {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!copied) return
    const timeout = setTimeout(() => setCopied(false), 2000)
    return () => clearTimeout(timeout)
  }, [copied])

  const label = copied ? t("chat.copied") : t("chat.copy")

  return (
    <Button
      type="button"
      size="icon-sm"
      variant="ghost"
      aria-label={label}
      title={label}
      className={cn("text-muted-foreground", className)}
      onClick={() => {
        navigator.clipboard.writeText(value).then(
          () => setCopied(true),
          () => toast.error(t("chat.copyFailed")),
        )
      }}
    >
      {copied ? (
        <CheckIcon className="size-3.5 text-emerald-600" />
      ) : (
        <CopyIcon className="size-3.5" />
      )}
    </Button>
  )
}
