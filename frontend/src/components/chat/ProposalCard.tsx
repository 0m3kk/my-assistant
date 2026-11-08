import { useState } from "react"
import { BotIcon, WrenchIcon } from "lucide-react"

import { useAuth } from "@/lib/auth/AuthContext"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ProposalReviewDialog } from "@/components/studio/ProposalReviewDialog"

export interface ProposalSummary {
  proposal_id: string
  status: "pending" | "applied" | "rejected"
  resource: "tool" | "agent"
  action: "create" | "update"
  title: string
}

function StatusBadge({ status }: { status: string }) {
  const variant = status === "applied" ? "default" : status === "rejected" ? "destructive" : "secondary"
  return <Badge variant={variant}>{status}</Badge>
}

/** Inline card rendered from `meta.propose_tool` / `meta.propose_agent` output. */
export function ProposalCard({ data }: { data: unknown }) {
  const p = data as ProposalSummary | null
  const { user } = useAuth()
  const [open, setOpen] = useState(false)
  if (!p?.proposal_id) return null
  const isTool = p.resource === "tool"
  return (
    <div className="my-1 max-w-xl rounded-lg border bg-card p-3">
      <div className="flex items-start justify-between gap-3">
        <div className="flex items-start gap-2">
          <span className="mt-0.5 text-muted-foreground">
            {isTool ? <WrenchIcon className="size-4" /> : <BotIcon className="size-4" />}
          </span>
          <div>
            <p className="text-sm font-medium">
              {p.action === "create" ? "New" : "Update"} {p.resource}: {p.title}
            </p>
            <p className="text-xs text-muted-foreground">
              Proposal {p.proposal_id.slice(0, 8)} · waiting for review
            </p>
          </div>
        </div>
        <StatusBadge status={p.status} />
      </div>
      {user?.role === "admin" && p.status === "pending" && (
        <div className="mt-3 flex justify-end">
          <Button size="sm" onClick={() => setOpen(true)}>
            Review &amp; apply
          </Button>
        </div>
      )}
      <ProposalReviewDialog proposalId={p.proposal_id} open={open} onOpenChange={setOpen} />
    </div>
  )
}
