import { useState } from "react"
import { useTranslation } from "react-i18next"
import { CircleCheckIcon, CircleXIcon, LoaderCircleIcon, ShieldAlertIcon } from "lucide-react"

import type { Approval, FeedbackRow, ToolUIPartState } from "@/lib/types"
import { cn } from "@/lib/utils"
import { CodeBlock, CodeBlockContent } from "@/components/ui/code-block"
import {
  Trace,
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace"
import { FeedbackControl, findToolFeedback } from "./FeedbackControl"
import { toolDetailRenderer } from "./registries"

export interface ToolPartData {
  toolCallId: string
  toolName: string
  state: ToolUIPartState
  input: unknown
  output?: unknown
  errorText?: string
  approved?: boolean
}

const TOOL_LABELS: Record<string, string> = {
  "workspace.read": "Read workspace file",
  "workspace.write": "Write workspace file",
  "workspace.list": "List workspace",
  current_time: "Current time",
  web_search: "Web search",
  save_artifact: "Save artifact",
  switch_agent: "Switch agent",
}

function summary(part: ToolPartData): string {
  const label = TOOL_LABELS[part.toolName] ?? part.toolName
  const input = part.input as Record<string, unknown> | null | undefined
  if (part.toolName === "web_search") {
    const query = typeof input?.query === "string" ? input.query : ""
    const count = (part.output as { results?: unknown[] } | undefined)?.results?.length
    const base = query ? `${label} “${query}”` : label
    return count != null ? `${base} · ${count}` : base
  }
  if (part.toolName === "workspace.read" || part.toolName === "workspace.write") {
    return `${label} ${stripWorkspacePrefix(String(input?.path ?? ""))}`
  }
  if (input?.path) return `${label} ${stripWorkspacePrefix(String(input.path))}`
  return label
}

/**
 * Hide the sandbox prefix: display paths relative to the conversation
 * workspace (`…/workspace/<conversation>/sub/file` → `sub/file`).
 */
function stripWorkspacePrefix(value: string): string {
  return value.replace(/[^"\s]*[\\/]workspace[\\/][^\\/\s"]*[\\/]?/g, "")
}

/** Rewrite only object keys named `path`; never touch file contents. */
function scrubPaths(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(scrubPaths)
  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, entry]) => [
        key,
        key === "path" && typeof entry === "string"
          ? stripWorkspacePrefix(entry)
          : scrubPaths(entry),
      ]),
    )
  }
  return value
}

export function ToolPartLine({
  part,
  conversationId,
  feedback,
  approval,
}: {
  part: ToolPartData
  conversationId: string
  feedback?: FeedbackRow[]
  approval?: Approval | null
}) {
  const { t } = useTranslation()
  const [open, setOpen] = useState<boolean | undefined>(undefined)

  const denied = part.state === "output-denied" || (part.state === "approval-responded" && part.approved === false)
  const failed = part.state === "output-error"
  const done = part.state === "output-available" || failed || denied

  const title = denied
    ? `${t("tool.denied")} ${summary(part)}`
    : failed
      ? `${t("tool.failed")} ${summary(part)}`
      : done
        ? `${t("tool.ran")} ${summary(part)}`
        : `${t("tool.running")} ${summary(part)}`

  const outputText = (() => {
    if (failed) return part.errorText ?? "Unknown error"
    if (part.output == null) return ""
    if (typeof part.output === "string") return part.output
    return JSON.stringify(scrubPaths(part.output), null, 2)
  })()

  const initial = findToolFeedback(feedback, part.toolCallId)
  const detail = done && part.output != null ? toolDetailRenderer(part.toolName) : undefined
  const approvalLabel = approval
    ? approval.status === "approved"
      ? t("approval.approved")
      : approval.status === "denied"
        ? t("approval.denied")
        : t("approval.pending")
    : null

  if (!done) {
    return (
      <Trace className="group/tool max-w-3xl rounded-md border px-3 py-2 text-sm">
        <TraceLine data-status={failed ? "error" : undefined} className="text-muted-foreground">
          <TraceIcon>
            <LoaderCircleIcon className="animate-spin" />
          </TraceIcon>
          <TraceTitle>{title}</TraceTitle>
        </TraceLine>
      </Trace>
    )
  }

  return (
    <Trace className="group/tool max-w-3xl rounded-md border px-3 py-2 text-sm">
      <TraceCollapsible open={open} onOpenChange={setOpen}>
        <TraceCollapsibleTrigger
          data-status={denied ? undefined : failed ? "error" : "success"}
          className="w-full"
        >
          <TraceIcon>
            {denied ? <ShieldAlertIcon /> : failed ? <CircleXIcon /> : <CircleCheckIcon />}
          </TraceIcon>
          <TraceTitle className={cn("flex-1", denied && "text-muted-foreground line-through")}>
            {title}
          </TraceTitle>
          {approval && (
            <span
              className="shrink-0 text-xs text-muted-foreground"
              title={approval.decided_at ? new Date(approval.decided_at).toLocaleString() : undefined}
            >
              · {approvalLabel}
            </span>
          )}
        </TraceCollapsibleTrigger>
        {detail || outputText || approval ? (
          <TraceCollapsibleContent>
            {approval && (
              <div className="mb-2 flex flex-wrap items-center gap-x-2 gap-y-1 rounded-sm bg-muted/40 px-2 py-1 text-xs text-muted-foreground">
                <span className="font-medium text-foreground">{approvalLabel}</span>
                {approval.requested_resource && (
                  <code className="max-w-full truncate font-mono">{approval.requested_resource}</code>
                )}
                {approval.decided_at && (
                  <span className="ml-auto tabular-nums">
                    {new Date(approval.decided_at).toLocaleString()}
                  </span>
                )}
              </div>
            )}
            {detail
              ? detail(part.output, part.input)
              : outputText
                ? (
                    <CodeBlock size="sm">
                      <CodeBlockContent>{outputText}</CodeBlockContent>
                    </CodeBlock>
                  )
                : null}
          </TraceCollapsibleContent>
        ) : null}
      </TraceCollapsible>
      <div className="pointer-events-none grid grid-rows-[0fr] pl-6 opacity-0 transition-[grid-template-rows,opacity] duration-150 group-hover/tool:pointer-events-auto group-hover/tool:grid-rows-[1fr] group-hover/tool:opacity-100 focus-within:pointer-events-auto focus-within:grid-rows-[1fr] focus-within:opacity-100">
        <div className="min-h-0 overflow-hidden">
          <FeedbackControl
            conversationId={conversationId}
            scope="tool_run"
            toolCallId={part.toolCallId}
            initial={initial}
          />
        </div>
      </div>
    </Trace>
  )
}
