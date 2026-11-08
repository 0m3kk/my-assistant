import { useEffect, useState } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { toast } from "sonner"

import { adminApi } from "@/lib/api/endpoints"
import type { ToolRow } from "@/lib/types"
import { ApiError } from "@/lib/api/client"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

function configString(config: Record<string, unknown>, key: string): string {
  const value = config[key]
  return typeof value === "string" ? value : ""
}

/** SearXNG settings for the builtin `web_search` tool (stored in tools.config). */
export function WebSearchSettingsDialog({
  tool,
  open,
  onOpenChange,
}: {
  tool: ToolRow | null
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const queryClient = useQueryClient()
  const [baseUrl, setBaseUrl] = useState("")
  const [maxResults, setMaxResults] = useState(5)
  const [language, setLanguage] = useState("")
  const [categories, setCategories] = useState("")
  const [safesearch, setSafesearch] = useState("default")
  const [timeRange, setTimeRange] = useState("any")
  const [timeoutMs, setTimeoutMs] = useState(15000)

  useEffect(() => {
    if (!open || !tool) return
    const config = tool.config ?? {}
    setBaseUrl(configString(config, "base_url"))
    setMaxResults(Number(config.max_results) || 5)
    setLanguage(configString(config, "language"))
    setCategories(configString(config, "categories"))
    setSafesearch(config.safesearch != null ? String(config.safesearch) : "default")
    setTimeRange(configString(config, "time_range") || "any")
    setTimeoutMs(Number(config.timeout_ms) || 15000)
  }, [open, tool])

  const save = useMutation({
    mutationFn: () => {
      if (!tool) throw new Error("no tool")
      const config: Record<string, unknown> = { ...(tool.config ?? {}) }
      const setOrDelete = (key: string, value: unknown) => {
        if (value === "" || value == null) delete config[key]
        else config[key] = value
      }
      setOrDelete("base_url", baseUrl.trim())
      setOrDelete("language", language.trim())
      setOrDelete("categories", categories.trim())
      setOrDelete("safesearch", safesearch === "default" ? "" : Number(safesearch))
      setOrDelete("time_range", timeRange === "any" ? "" : timeRange)
      config.max_results = Math.min(20, Math.max(1, Math.round(maxResults) || 5))
      config.timeout_ms = Math.min(120000, Math.max(1000, Math.round(timeoutMs) || 15000))
      return adminApi.tools.update(tool.id, { config })
    },
    onSuccess: () => {
      toast.success("Web search settings saved")
      void queryClient.invalidateQueries({ queryKey: ["admin", "tools"] })
      onOpenChange(false)
    },
    onError: (e) => toast.error(e instanceof ApiError ? e.message : String(e)),
  })

  if (!tool) return null

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[88dvh] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Web search (SearXNG)</DialogTitle>
        </DialogHeader>
        <div className="grid gap-4">
          <div className="grid gap-2">
            <Label>SearXNG base URL</Label>
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="Empty = SEARXNG_URL from the environment"
            />
            <p className="text-xs text-muted-foreground">
              The instance must expose the JSON API (<code>json</code> in{" "}
              <code>search.formats</code>).
            </p>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <div className="grid gap-2">
              <Label>Max results (1–20)</Label>
              <Input
                type="number"
                min={1}
                max={20}
                value={maxResults}
                onChange={(e) => setMaxResults(Number(e.target.value))}
              />
            </div>
            <div className="grid gap-2">
              <Label>Timeout (ms)</Label>
              <Input
                type="number"
                min={1000}
                max={120000}
                value={timeoutMs}
                onChange={(e) => setTimeoutMs(Number(e.target.value))}
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <div className="grid gap-2">
              <Label>Language (optional)</Label>
              <Input
                value={language}
                onChange={(e) => setLanguage(e.target.value)}
                placeholder="en, vi, all"
              />
            </div>
            <div className="grid gap-2">
              <Label>Categories (optional)</Label>
              <Input
                value={categories}
                onChange={(e) => setCategories(e.target.value)}
                placeholder="general,news"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-2">
            <div className="grid gap-2">
              <Label>Safe search</Label>
              <Select value={safesearch} onValueChange={setSafesearch}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="default">Instance default</SelectItem>
                  <SelectItem value="0">Off</SelectItem>
                  <SelectItem value="1">Moderate</SelectItem>
                  <SelectItem value="2">Strict</SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label>Time range</Label>
              <Select value={timeRange} onValueChange={setTimeRange}>
                <SelectTrigger>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="any">Any</SelectItem>
                  <SelectItem value="day">Past day</SelectItem>
                  <SelectItem value="week">Past week</SelectItem>
                  <SelectItem value="month">Past month</SelectItem>
                  <SelectItem value="year">Past year</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={() => save.mutate()} disabled={save.isPending}>
            Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
