import { useQuery } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import { CircleCheckIcon, CircleXIcon, ShieldAlertIcon } from "lucide-react"

import { approvalsApi, conversationsApi } from "@/lib/api/endpoints"
import { Button } from "@/components/ui/button"
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item"
import { CodeBlock, CodeBlockContent } from "@/components/ui/code-block"

interface ApprovalCardProps {
  approvalId: string
  conversationId: string
  toolName: string
  input: unknown
  onDecided: (approvalId: string, approved: boolean) => void
}

export function ApprovalCard({
  approvalId,
  conversationId,
  toolName,
  input,
  onDecided,
}: ApprovalCardProps) {
  const { t } = useTranslation()
  const approvals = useQuery({
    queryKey: ["approvals", conversationId],
    queryFn: () => conversationsApi.approvals(conversationId),
    enabled: !!conversationId,
    staleTime: 5_000,
  })
  const approval = approvals.data?.find((a) => a.id === approvalId)
  const outOfRoot = approval?.reason === "out_of_root"

  async function decide(approved: boolean, mode?: "once" | "propose_update") {
    // Ledger write first (server is the source of truth) and awaited, so the
    // resumed chat request sees the decision; then let useChat resend the
    // messages with the approval response attached.
    try {
      await approvalsApi.decide(approvalId, approved ? "approved" : "denied", mode)
    } catch {
      // The chat resume still reconciles against the ledger.
    }
    onDecided(approvalId, approved)
  }

  return (
    <Item variant="outline" className="my-1 w-full max-w-full">
      <ItemMedia variant="icon">
        <ShieldAlertIcon className="text-amber-500" />
      </ItemMedia>
      <ItemContent className="min-w-0">
        <ItemTitle>{t("approval.title")}</ItemTitle>
        <ItemDescription>
          <span className="font-medium text-foreground">{toolName}</span> —{" "}
          {outOfRoot ? t("approval.outOfRoot") : t("approval.description")}
        </ItemDescription>
        {outOfRoot && approval?.requested_resource && (
          <p className="mt-1 font-mono text-xs break-all text-amber-700 dark:text-amber-300">
            {approval.requested_resource}
          </p>
        )}
        {input != null && (
          <div className="mt-2 max-h-40 w-full max-w-full min-w-0 overflow-auto overscroll-contain rounded-md border">
            <CodeBlock size="sm">
              <CodeBlockContent>{JSON.stringify(input, null, 2)}</CodeBlockContent>
            </CodeBlock>
          </div>
        )}
      </ItemContent>
      <ItemActions className="flex-wrap justify-end">
        <Button size="sm" variant="outline" onClick={() => decide(false)}>
          <CircleXIcon /> {t("approval.deny")}
        </Button>
        {outOfRoot ? (
          <>
            <Button size="sm" variant="secondary" onClick={() => decide(true, "once")}>
              {t("approval.allowOnce")}
            </Button>
            <Button size="sm" onClick={() => decide(true, "propose_update")}>
              <CircleCheckIcon /> {t("approval.allowAndPropose")}
            </Button>
          </>
        ) : (
          <Button size="sm" onClick={() => decide(true)}>
            <CircleCheckIcon /> {t("approval.approve")}
          </Button>
        )}
      </ItemActions>
    </Item>
  )
}
