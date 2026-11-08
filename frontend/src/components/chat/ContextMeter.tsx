import type { UIMessage } from "ai"
import { useTranslation } from "react-i18next"

import { messageUsage, type UsageSnapshot } from "@/lib/types"
import { cn, formatContextWindow, formatTokens } from "@/lib/utils"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

/** Latest usage snapshot in the transcript (streamed or hydrated). */
function latestUsage(messages: UIMessage[]): UsageSnapshot | null {
  for (let i = messages.length - 1; i >= 0; i--) {
    const usage = messageUsage(messages[i])
    if (usage) return usage
  }
  return null
}

/**
 * Context-window meter for the composer bar.
 *
 * Shows how full the model's context was on the last turn: the final provider
 * call's prompt tokens against the model's configured context window. Renders
 * nothing until a provider has reported usage, so a model that never returns
 * `usage` never leaves an empty widget behind.
 */
export function ContextMeter({ messages }: { messages: UIMessage[] }) {
  const { t } = useTranslation()
  const usage = latestUsage(messages)

  if (usage == null || usage.contextTokens == null || usage.contextTokens <= 0) return null

  const used = usage.contextTokens
  const window = usage.contextWindow
  const ratio = window ? Math.min(used / window, 1) : null
  const percent = ratio != null ? Math.round(ratio * 100) : null
  // Warn once past 80% so a long conversation doesn't hit the wall silently.
  const tone =
    ratio == null
      ? "text-muted-foreground"
      : ratio >= 0.9
        ? "text-destructive"
        : ratio >= 0.8
          ? "text-amber-500"
          : "text-primary"

  // Ring geometry — the colored arc is the used fraction of the full circle.
  const size = 18
  const stroke = 2.5
  const radius = (size - stroke) / 2
  const circumference = 2 * Math.PI * radius
  const arc = ratio != null ? circumference * ratio : 0

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            className="flex cursor-default items-center justify-center rounded-md bg-muted/60 p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            aria-label={
              percent != null ? `${t("chat.context.title")}: ${percent}%` : t("chat.context.title")
            }
          >
            <svg
              width={size}
              height={size}
              viewBox={`0 0 ${size} ${size}`}
              className="-rotate-90"
              aria-hidden
            >
              <circle
                cx={size / 2}
                cy={size / 2}
                r={radius}
                fill="none"
                strokeWidth={stroke}
                stroke="currentColor"
                className="text-muted-foreground/40"
              />
              {ratio != null && (
                <circle
                  cx={size / 2}
                  cy={size / 2}
                  r={radius}
                  fill="none"
                  strokeWidth={stroke}
                  strokeLinecap="round"
                  stroke="currentColor"
                  strokeDasharray={circumference}
                  strokeDashoffset={circumference - arc}
                  className={cn("transition-[stroke-dashoffset]", tone)}
                />
              )}
            </svg>
          </div>
        </TooltipTrigger>
        <TooltipContent side="top">
          <ul className="grid list-disc gap-0.5 pl-4">
            <li>
              {window
                ? t("chat.context.usedOf", {
                    used: formatTokens(used),
                    total: formatContextWindow(window),
                    percent,
                  })
                : t("chat.context.used", { used: formatTokens(used) })}
            </li>
            {usage.completionTokens != null && (
              <li>{t("chat.context.output", { tokens: formatTokens(usage.completionTokens) })}</li>
            )}
            {usage.promptTokens != null && (
              <li>{t("chat.context.totalPrompt", { tokens: formatTokens(usage.promptTokens) })}</li>
            )}
          </ul>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
