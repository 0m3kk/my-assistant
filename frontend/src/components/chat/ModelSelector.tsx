import { useMemo } from "react"
import { useQuery } from "@tanstack/react-query"
import { BrainIcon, ChevronsUpDownIcon, SparklesIcon } from "lucide-react"
import { useTranslation } from "react-i18next"

import { modelsApi } from "@/lib/api/endpoints"
import { REASONING_EFFORT_LEVELS, type ModelOption } from "@/lib/types"
import { formatContextWindow } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"

interface ModelSelectorProps {
  value: string | null
  /** Explicit per-request override; null = follow the model's configured default. */
  effort: string | null
  onChange: (modelId: string) => void
  onEffortChange: (effort: string | null) => void
}

/** One selectable model: name, wire name, and its context window when known. */
function ModelItem({ model, onChange }: { model: ModelOption; onChange: (id: string) => void }) {
  return (
    <DropdownMenuRadioItem
      value={model.id}
      // Selected model gets the same clear treatment as the sidebar's active
      // conversation: primary-tinted background, not just the dot.
      className="py-1.5 text-xs data-[state=checked]:bg-primary/10 data-[state=checked]:text-foreground"
      onClick={() => onChange(model.id)}
    >
      <span className="flex min-w-0 flex-col items-start">
        <span className="flex w-full items-center gap-2">
          <span className="truncate font-medium">{model.display_name}</span>
          {model.context_window != null && (
            <span className="ml-auto shrink-0 text-[0.65rem] text-muted-foreground">
              {formatContextWindow(model.context_window)}
            </span>
          )}
        </span>
        <span className="truncate text-[0.7rem] text-muted-foreground">{model.api_name}</span>
      </span>
    </DropdownMenuRadioItem>
  )
}

export function ModelSelector({ value, effort, onChange, onEffortChange }: ModelSelectorProps) {
  const { t } = useTranslation()
  const { data: models } = useQuery({ queryKey: ["models"], queryFn: () => modelsApi.list() })

  const selected: ModelOption | undefined =
    models?.find((m) => m.id === value) ?? models?.find((m) => m.is_default) ?? models?.[0]

  // Effective effort: explicit override → the model's configured default.
  const effectiveEffort = effort ?? selected?.reasoning_effort ?? null

  // Group by provider so multi-provider installs stay readable.
  const groups = useMemo(() => {
    const byProvider = new Map<string, ModelOption[]>()
    for (const model of models ?? []) {
      const key = model.provider_name || t("chat.model")
      const bucket = byProvider.get(key)
      if (bucket) bucket.push(model)
      else byProvider.set(key, [model])
    }
    return [...byProvider.entries()]
  }, [models, t])
  const showGroups = groups.length > 1

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        {/* Lives in the composer's bottom row now: compact text, full width up
            to the action buttons, the model name truncates last. */}
        <Button
          variant="outline"
          size="sm"
          className="h-8 max-w-full min-w-0 gap-2 border-primary/30 bg-primary/10 text-xs text-foreground hover:bg-primary/15 dark:bg-primary/10 dark:hover:bg-primary/15 [&_svg:not([class*='size-'])]:size-3.5"
        >
          <SparklesIcon className="shrink-0 text-primary" />
          <span className="min-w-0 flex-1 truncate text-left">{selected ? selected.display_name : "…"}</span>
          {effectiveEffort && (
            <span className="flex shrink-0 items-center gap-1 rounded-sm bg-background px-1.5 py-0.5 text-[0.7rem] text-muted-foreground">
              <BrainIcon className="size-3" />
              {t(`chat.effort.${effectiveEffort}`)}
            </span>
          )}
          <ChevronsUpDownIcon className="shrink-0 text-foreground/60" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-auto text-xs">
        <DropdownMenuLabel className="text-xs">{t("chat.model")}</DropdownMenuLabel>
        <DropdownMenuSeparator />
        {showGroups ? (
          groups.map(([providerName, group], groupIndex) => (
            <div key={providerName}>
              {groupIndex > 0 && <DropdownMenuSeparator />}
              <DropdownMenuLabel className="text-[0.7rem] font-normal text-muted-foreground">
                {providerName}
              </DropdownMenuLabel>
              <DropdownMenuRadioGroup value={selected?.id ?? ""} className="grid gap-0.5">
                {group.map((m) => (
                  <ModelItem key={m.id} model={m} onChange={onChange} />
                ))}
              </DropdownMenuRadioGroup>
            </div>
          ))
        ) : (
          <DropdownMenuRadioGroup value={selected?.id ?? ""} className="grid gap-0.5">
            {(models ?? []).map((m) => (
              <ModelItem key={m.id} model={m} onChange={onChange} />
            ))}
          </DropdownMenuRadioGroup>
        )}
        {models?.length === 0 && (
          <div className="px-2 py-3 text-xs text-muted-foreground">{t("chat.noModels")}</div>
        )}
        <DropdownMenuSeparator />
        {/* Reasoning effort as a submenu — shows the effective effort badge. */}
        <DropdownMenuSub>
          <DropdownMenuSubTrigger className="gap-2 text-xs">
            <BrainIcon className="size-3.5" />
            <span>{t("chat.effort.title")}</span>
            {effectiveEffort && (
              <span className="ml-auto rounded-sm bg-muted px-1.5 py-0.5 text-[0.7rem] font-normal text-muted-foreground">
                {t(`chat.effort.${effectiveEffort}`)}
              </span>
            )}
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="w-36 text-xs">
            <DropdownMenuRadioGroup value={effort ?? "default"}>
              {/* "Default" clears the override — follow the model's configured effort. */}
              <DropdownMenuRadioItem
                value="default"
                className="py-1.5 text-xs"
                onClick={() => onEffortChange(null)}
              >
                {t("chat.effort.default")}
              </DropdownMenuRadioItem>
              {REASONING_EFFORT_LEVELS.map((e) => (
                <DropdownMenuRadioItem
                  key={e}
                  value={e}
                  className="py-1.5 text-xs"
                  onClick={() => onEffortChange(e)}
                >
                  {t(`chat.effort.${e}`)}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
