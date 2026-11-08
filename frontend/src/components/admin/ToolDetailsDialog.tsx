import { useEffect, useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"
import { HammerIcon, PlayIcon } from "lucide-react"

import { toolOpsApi, type VersionRow } from "@/lib/api/admin-extra"
import { ApiError } from "@/lib/api/client"
import type { ToolRow } from "@/lib/types"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Separator } from "@/components/ui/separator"
import { Textarea } from "@/components/ui/textarea"

export function ToolDetailsDialog({
  tool,
  open,
  onOpenChange,
}: {
  tool: ToolRow | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const queryClient = useQueryClient()
  const enabled = open && !!tool
  const toolId = tool?.id ?? ""

  const builds = useQuery({
    queryKey: ["builds", toolId],
    queryFn: () => toolOpsApi.builds(toolId),
    enabled,
  })
  const versions = useQuery({
    queryKey: ["versions", "tool", toolId],
    queryFn: () => toolOpsApi.versions(toolId),
    enabled,
  })

  const [testInput, setTestInput] = useState('{\n  "hello": "world"\n}')
  const [testResult, setTestResult] = useState("")

  useEffect(() => {
    setTestResult("")
  }, [toolId])

  const build = useMutation({
    mutationFn: () => toolOpsApi.build(toolId),
    onSuccess: () => {
      toast.success("Build finished")
      void queryClient.invalidateQueries({ queryKey: ["builds", toolId] })
      void queryClient.invalidateQueries({ queryKey: ["admin", "tools"] })
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const test = useMutation({
    mutationFn: () => {
      let input: unknown
      try {
        input = JSON.parse(testInput)
      } catch {
        throw new Error("test input is not valid JSON")
      }
      return toolOpsApi.test(toolId, input)
    },
    onSuccess: (r) => setTestResult(JSON.stringify(r, null, 2)),
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  const restore = useMutation({
    mutationFn: (revision: number) => toolOpsApi.restore(toolId, revision),
    onSuccess: () => {
      toast.success("Restore proposal created — review it in Studio")
      void queryClient.invalidateQueries({ queryKey: ["proposals"] })
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  if (!tool) return null
  const latest = builds.data?.builds?.[0]

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <span className="font-mono text-sm">{tool.name}</span>
            <Badge variant="secondary">{tool.kind}</Badge>
            <Badge variant={tool.status === "enabled" ? "default" : "outline"}>{tool.status}</Badge>
            <Badge variant="outline">r{tool.revision}</Badge>
          </DialogTitle>
        </DialogHeader>

        <p className="text-sm text-muted-foreground">{tool.description}</p>

        {tool.kind === "wasm" && (
          <section className="grid gap-2">
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-semibold">WASM artifact</h3>
              <Button size="sm" onClick={() => build.mutate()} disabled={build.isPending}>
                <HammerIcon /> {build.isPending ? "Building…" : "Build"}
              </Button>
            </div>
            {latest ? (
              <div className="grid gap-1 text-sm">
                <div className="flex flex-wrap items-center gap-2">
                  <Badge variant={latest.status === "built" ? "default" : latest.status === "failed" ? "destructive" : "secondary"}>
                    {latest.status}
                  </Badge>
                  <span className="text-xs text-muted-foreground">revision {latest.revision}</span>
                  {latest.sha256 && (
                    <span className="font-mono text-xs text-muted-foreground">
                      sha256 {latest.sha256.slice(0, 12)}…
                    </span>
                  )}
                  {latest.size_bytes != null && (
                    <span className="text-xs text-muted-foreground">
                      {(latest.size_bytes / 1024).toFixed(1)} KB
                    </span>
                  )}
                </div>
                {latest.build_log && (
                  <details className="rounded-md border p-2">
                    <summary className="cursor-pointer text-xs text-muted-foreground">Build log</summary>
                    <pre className="mt-2 max-h-60 overflow-auto text-xs">{latest.build_log}</pre>
                  </details>
                )}
              </div>
            ) : (
              <p className="text-xs text-muted-foreground">No build yet.</p>
            )}

            <Separator />
            <div className="grid gap-2">
              <h3 className="text-sm font-semibold">Test run</h3>
              <p className="text-xs text-muted-foreground">
                Runs the built artifact without secrets and with filesystem writes disabled.
              </p>
              <Textarea
                value={testInput}
                onChange={(e) => setTestInput(e.target.value)}
                rows={5}
                className="font-mono text-xs"
              />
              <div className="flex justify-end">
                <Button size="sm" variant="secondary" onClick={() => test.mutate()} disabled={test.isPending}>
                  <PlayIcon /> {test.isPending ? "Running…" : "Run test"}
                </Button>
              </div>
              {testResult && (
                <pre className="max-h-60 overflow-auto rounded-md border bg-muted/30 p-2 text-xs">
                  {testResult}
                </pre>
              )}
            </div>
          </section>
        )}

        <Separator />
        <section className="grid gap-2">
          <h3 className="text-sm font-semibold">Version history</h3>
          {(versions.data?.versions ?? []).map((v: VersionRow) => (
            <div key={v.revision} className="flex items-center justify-between rounded-md border p-2">
              <div>
                <div className="flex items-center gap-2">
                  <Badge variant="secondary">r{v.revision}</Badge>
                  <span className="text-sm">{v.note || "version"}</span>
                </div>
                <p className="text-xs text-muted-foreground">
                  {new Date(v.created_at).toLocaleString()}
                </p>
              </div>
              <Button size="sm" variant="outline" disabled={restore.isPending} onClick={() => restore.mutate(v.revision)}>
                Restore
              </Button>
            </div>
          ))}
          {versions.data?.versions.length === 0 && (
            <p className="text-xs text-muted-foreground">
              No versions yet. Versions are written when a proposal is applied.
            </p>
          )}
        </section>
      </DialogContent>
    </Dialog>
  )
}
