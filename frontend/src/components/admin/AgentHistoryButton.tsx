import { useState } from "react"
import { HistoryIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { VersionHistoryDialog } from "./VersionHistoryDialog"

export function AgentHistoryButton({ agentId, name }: { agentId: string; name: string }) {
  const [open, setOpen] = useState(false)
  return (
    <>
      <Button variant="outline" size="sm" onClick={() => setOpen(true)}>
        <HistoryIcon /> History
      </Button>
      <VersionHistoryDialog
        resource="agent"
        targetId={agentId}
        title={name}
        open={open}
        onOpenChange={setOpen}
      />
    </>
  )
}
