import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"

import { toolOpsApi, type VersionRow } from "@/lib/api/admin-extra"
import { ApiError } from "@/lib/api/client"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"

export function VersionHistoryDialog({
  resource,
  targetId,
  title,
  open,
  onOpenChange,
}: {
  resource: "tool" | "agent"
  targetId: string
  title: string
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const queryClient = useQueryClient()
  const versions = useQuery({
    queryKey: ["versions", resource, targetId],
    queryFn: () =>
      resource === "tool"
        ? toolOpsApi.versions(targetId)
        : toolOpsApi.agentVersions(targetId),
    enabled: open,
  })

  const restore = useMutation({
    mutationFn: (revision: number) =>
      resource === "tool"
        ? toolOpsApi.restore(targetId, revision)
        : toolOpsApi.restoreAgent(targetId, revision),
    onSuccess: () => {
      toast.success("Restore proposal created — review it in Studio")
      void queryClient.invalidateQueries({ queryKey: ["proposals"] })
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[85dvh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{title} — version history</DialogTitle>
        </DialogHeader>
        <div className="grid gap-2">
          {(versions.data?.versions ?? []).map((v: VersionRow) => (
            <div key={v.revision} className="flex items-center justify-between rounded-md border p-2">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <Badge variant="secondary">r{v.revision}</Badge>
                  <span className="text-sm">{v.note || "version"}</span>
                </div>
                <p className="text-xs text-muted-foreground">
                  {new Date(v.created_at).toLocaleString()}
                </p>
              </div>
              <Button
                size="sm"
                variant="outline"
                disabled={restore.isPending}
                onClick={() => restore.mutate(v.revision)}
              >
                Restore
              </Button>
            </div>
          ))}
          {versions.data?.versions.length === 0 && (
            <p className="text-sm text-muted-foreground">
              No versions yet. Versions are written when a proposal is applied.
            </p>
          )}
        </div>
      </DialogContent>
    </Dialog>
  )
}
