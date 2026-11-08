import { useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { useParams } from "react-router-dom"
import { useTranslation } from "react-i18next"
import { ArrowLeftIcon, BotIcon, WrenchIcon } from "lucide-react"

import { adminApi } from "@/lib/api/endpoints"
import type { ChangeProposal, FeedbackRow } from "@/lib/types"
import { Button } from "@/components/ui/button"
import { Badge } from "@/components/ui/badge"
import { Separator } from "@/components/ui/separator"
import { MetaChat } from "@/components/studio/MetaChat"
import { ProposalReviewDialog } from "@/components/studio/ProposalReviewDialog"

const META_AGENTS = [
  { slug: "tool-writer", label: "Tool Writer" },
  { slug: "tool-improver", label: "Tool Improver" },
  { slug: "agent-writer", label: "Agent Writer" },
  { slug: "agent-improver", label: "Agent Improver" },
] as const

export function StudioPage() {
  const { t } = useTranslation()
  const { meta } = useParams()
  const [active, setActive] = useState<string>(meta ?? "tool-writer")
  const [starter, setStarter] = useState<string | undefined>()
  const [reviewId, setReviewId] = useState<string | null>(null)

  const proposals = useQuery({
    queryKey: ["proposals", "pending"],
    queryFn: () => adminApi.proposals.list({ status: "pending" }).then((r) => r.proposals),
    staleTime: 5_000,
  })
  const feedback = useQuery({
    queryKey: ["feedback", "open"],
    queryFn: () => adminApi.feedback.list({ status: "open" }).then((r) => r.feedback),
    staleTime: 5_000,
  })

  function improveFrom(f: FeedbackRow) {
    if (f.scope === "tool_run" && f.tool_id) {
      setActive("tool-improver")
      setStarter(
        `Improve tool ${f.tool_id}. Read its feedback (id ${f.id}) and recent executions, then propose the smallest change. Ask me if you need more context.`,
      )
      return
    }
    if (f.scope === "agent_run" && f.agent_id) {
      setActive("agent-improver")
      setStarter(
        `Improve agent ${f.agent_id}. Read its feedback (id ${f.id}), then propose the smallest change. Ask me if you need more context.`,
      )
    }
  }

  return (
    <div className="mx-auto flex min-h-dvh max-w-7xl flex-col p-6">
      <div className="flex items-center justify-between pb-3">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" onClick={() => history.back()} aria-label="Back">
            <ArrowLeftIcon />
          </Button>
          <h1 className="text-xl font-semibold">{t("studio.title")}</h1>
        </div>
        <span className="text-xs text-muted-foreground">{t("studio.subtitle")}</span>
      </div>

      <div className="flex flex-wrap gap-1 border-b pb-2">
        {META_AGENTS.map((m) => (
          <Button
            key={m.slug}
            size="sm"
            variant={active === m.slug ? "secondary" : "ghost"}
            onClick={() => {
              setActive(m.slug)
              setStarter(undefined)
            }}
          >
            {m.label}
          </Button>
        ))}
      </div>

      <div className="grid min-h-0 flex-1 gap-4 pt-4 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
        <div className="min-h-[60dvh] overflow-hidden rounded-lg border">
          <MetaChat key={`${active}:${starter ?? ""}`} agentSlug={active} starter={starter} />
        </div>

        <aside className="grid content-start gap-4">
          <section className="rounded-lg border p-3">
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-sm font-semibold">{t("studio.pendingProposals")}</h2>
              <Badge variant="secondary">{proposals.data?.length ?? 0}</Badge>
            </div>
            <div className="grid gap-2">
              {(proposals.data ?? []).map((p: ChangeProposal) => (
                <div key={p.id} className="rounded-md border p-2 text-sm">
                  <div className="flex items-center justify-between gap-2">
                    <span className="flex min-w-0 items-center gap-1.5">
                      {p.resource === "tool" ? (
                        <WrenchIcon className="size-3.5 shrink-0 text-muted-foreground" />
                      ) : (
                        <BotIcon className="size-3.5 shrink-0 text-muted-foreground" />
                      )}
                      <span className="truncate">
                        {p.action} {p.resource}: {p.title}
                      </span>
                    </span>
                    <Button size="sm" variant="outline" onClick={() => setReviewId(p.id)}>
                      {t("studio.review")}
                    </Button>
                  </div>
                  {p.rationale && (
                    <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{p.rationale}</p>
                  )}
                </div>
              ))}
              {proposals.data?.length === 0 && (
                <p className="text-xs text-muted-foreground">{t("studio.noProposals")}</p>
              )}
            </div>
          </section>

          <section className="rounded-lg border p-3">
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-sm font-semibold">{t("studio.openFeedback")}</h2>
              <Badge variant="secondary">{feedback.data?.length ?? 0}</Badge>
            </div>
            <div className="grid gap-2">
              {(feedback.data ?? []).map((f) => (
                <div key={f.id} className="rounded-md border p-2 text-sm">
                  <div className="flex items-center justify-between gap-2">
                    <span className="text-xs text-muted-foreground">
                      {f.scope === "tool_run" ? "Tool run" : "Agent run"} ·{" "}
                      {f.rating === 1 ? "👍" : "👎"}
                    </span>
                    <Button size="sm" variant="outline" onClick={() => improveFrom(f)}>
                      {t("studio.improve")}
                    </Button>
                  </div>
                  {f.comment && <p className="mt-1 whitespace-pre-wrap text-xs">{f.comment}</p>}
                </div>
              ))}
              {feedback.data?.length === 0 && (
                <p className="text-xs text-muted-foreground">{t("studio.noFeedback")}</p>
              )}
            </div>
          </section>

          <Separator />
          <p className="text-xs text-muted-foreground">{t("studio.hint")}</p>
        </aside>
      </div>

      {reviewId && (
        <ProposalReviewDialog
          proposalId={reviewId}
          open={reviewId !== null}
          onOpenChange={(o) => {
            if (!o) setReviewId(null)
          }}
        />
      )}
    </div>
  )
}
