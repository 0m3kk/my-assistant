import { useState } from "react"
import { MessageSquarePlusIcon, ThumbsDownIcon, ThumbsUpIcon } from "lucide-react"
import { toast } from "sonner"

import { feedbackApi } from "@/lib/api/endpoints"
import type { FeedbackRow } from "@/lib/types"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"

export interface FeedbackControlProps {
  conversationId: string
  scope: "tool_run" | "agent_run"
  runId?: string | null
  toolCallId?: string
  initial?: FeedbackRow | null
  className?: string
}

/**
 * Inline thumbs up/down + optional note. Backed by `/api/feedback`; the same
 * control is used for tool runs and assistant (agent) runs.
 */
export function FeedbackControl({
  conversationId,
  scope,
  runId,
  toolCallId,
  initial,
  className,
}: FeedbackControlProps) {
  const [rating, setRating] = useState<-1 | 1 | 0>(initial?.rating ?? 0)
  const [comment, setComment] = useState(initial?.comment ?? "")
  const [noteOpen, setNoteOpen] = useState(false)
  const [saving, setSaving] = useState(false)

  async function submit(next: -1 | 1, note = comment) {
    setSaving(true)
    try {
      const saved = await feedbackApi.submit({
        scope,
        conversation_id: conversationId,
        run_id: runId ?? undefined,
        tool_call_id: toolCallId,
        rating: next,
        comment: note,
      })
      setRating(saved.rating)
      setComment(saved.comment)
      toast.success("Feedback saved")
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className={cn("flex flex-wrap items-center gap-1.5 text-muted-foreground", className)}>
      <Button
        type="button"
        size="icon-sm"
        variant={rating === 1 ? "secondary" : "ghost"}
        disabled={saving}
        aria-label="Helpful"
        title="Helpful"
        onClick={() => void submit(1)}
      >
        <ThumbsUpIcon className={cn("size-3.5", rating === 1 && "text-emerald-600")} />
      </Button>
      <Button
        type="button"
        size="icon-sm"
        variant={rating === -1 ? "secondary" : "ghost"}
        disabled={saving}
        aria-label="Not helpful"
        title="Not helpful"
        onClick={() => void submit(-1)}
      >
        <ThumbsDownIcon className={cn("size-3.5", rating === -1 && "text-rose-600")} />
      </Button>
      <Button
        type="button"
        size="icon-sm"
        variant={noteOpen || comment ? "secondary" : "ghost"}
        aria-label="Add a note"
        title="Add a note"
        onClick={() => setNoteOpen((v) => !v)}
      >
        <MessageSquarePlusIcon className="size-3.5" />
      </Button>
      {noteOpen && (
        <div className="flex w-full max-w-md items-end gap-1.5">
          <Textarea
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="What should change?"
            rows={2}
            className="min-h-0 text-xs"
          />
          <Button
            type="button"
            size="sm"
            disabled={saving || rating === 0}
            onClick={() => void submit(rating === 0 ? 1 : (rating as -1 | 1))}
          >
            Save
          </Button>
        </div>
      )}
    </div>
  )
}

/** Match the current user's feedback for a tool call. */
export function findToolFeedback(
  feedback: FeedbackRow[] | undefined,
  toolCallId: string,
): FeedbackRow | undefined {
  return feedback?.find((f) => f.scope === "tool_run" && f.subject_key.endsWith(`:${toolCallId}`))
}

/** Match the current user's feedback for an agent run. */
export function findAgentFeedback(
  feedback: FeedbackRow[] | undefined,
  runId: string | null | undefined,
): FeedbackRow | undefined {
  if (!runId) return undefined
  return feedback?.find((f) => f.scope === "agent_run" && f.subject_key === `agent:${runId}`)
}
