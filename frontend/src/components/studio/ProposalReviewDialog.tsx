import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"

import { adminApi } from "@/lib/api/endpoints"
import { ApiError } from "@/lib/api/client"
import type { ChangeProposal } from "@/lib/types"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Textarea } from "@/components/ui/textarea"

export function ProposalReviewDialog({
  proposalId,
  open,
  onOpenChange,
}: {
  proposalId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const queryClient = useQueryClient()
  const proposal = useQuery({
    queryKey: ["proposal", proposalId],
    queryFn: () => adminApi.proposals.get(proposalId),
    enabled: open,
  })
  const [payloadText, setPayloadText] = useState("")
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (proposal.data) {
      setPayloadText(JSON.stringify(proposal.data.payload, null, 2))
      setError(null)
    }
  }, [proposal.data])

  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: ["proposals"] })
    void queryClient.invalidateQueries({ queryKey: ["proposal", proposalId] })
  }

  const apply = useMutation({
    mutationFn: () => {
      let payload: Record<string, unknown>
      try {
        payload = JSON.parse(payloadText) as Record<string, unknown>
      } catch {
        throw new Error("payload is not valid JSON")
      }
      return adminApi.proposals.apply(proposalId, { payload })
    },
    onSuccess: () => {
      toast.success("Proposal applied")
      invalidate()
      onOpenChange(false)
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : String(e)),
  })

  const reject = useMutation({
    mutationFn: () => adminApi.proposals.reject(proposalId),
    onSuccess: () => {
      toast.success("Proposal rejected")
      invalidate()
      onOpenChange(false)
    },
    onError: (e) => setError(e instanceof ApiError ? e.message : String(e)),
  })

  const p: ChangeProposal | undefined = proposal.data

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>
            Review {p?.resource} {p?.action}: {p?.title}
          </DialogTitle>
        </DialogHeader>
        {proposal.isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : (
          <div className="grid gap-4">
            {p?.rationale && (
              <div className="rounded-md border bg-muted/30 p-3 text-sm">
                <p className="mb-1 text-xs font-medium text-muted-foreground">Rationale</p>
                <p className="whitespace-pre-wrap">{p.rationale}</p>
              </div>
            )}
            <div className="grid gap-2">
              <p className="text-xs font-medium text-muted-foreground">
                Payload (edit before applying if needed)
              </p>
              <Textarea
                value={payloadText}
                onChange={(e) => setPayloadText(e.target.value)}
                rows={18}
                className="font-mono text-xs"
              />
            </div>
            {p?.apply_error && (
              <p className="text-xs text-rose-600">Last error: {p.apply_error}</p>
            )}
            {error && <p className="text-xs text-rose-600">{error}</p>}
          </div>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={() => reject.mutate()} disabled={reject.isPending}>
            Reject
          </Button>
          <Button onClick={() => apply.mutate()} disabled={apply.isPending || !p}>
            {apply.isPending ? "Applying…" : "Apply"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
