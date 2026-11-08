import { useQuery } from "@tanstack/react-query"
import { BotIcon } from "lucide-react"
import { useTranslation } from "react-i18next"

import { agentsApi } from "@/lib/api/endpoints"

/**
 * Read-only indicator of the agent every conversation runs on.
 *
 * Phase 1 exposes a single agent (`general`), which itself decides how to carry
 * out each task, so there is nothing to pick here.
 */
export function AgentBadge() {
  const { t } = useTranslation()
  const { data: agents } = useQuery({ queryKey: ["agents"], queryFn: () => agentsApi.list() })

  const agent = agents?.[0]
  if (!agent) return null

  return (
    <div
      title={agent.description}
      className="flex h-8 min-w-0 items-center gap-2 rounded-md border border-border bg-muted/40 px-2.5 text-xs text-foreground"
    >
      <BotIcon className="size-3.5 shrink-0 text-foreground/70" />
      <span className="min-w-0 truncate">{agent.name}</span>
      <span className="sr-only">{t("chat.agent")}</span>
    </div>
  )
}
