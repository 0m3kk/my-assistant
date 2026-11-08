import { useEffect, useMemo, useState } from "react"
import { useQuery } from "@tanstack/react-query"
import { useTranslation } from "react-i18next"
import {
  ChevronDownIcon,
  DownloadIcon,
  FileIcon,
  FolderOpenIcon,
} from "lucide-react"
import { toast } from "sonner"

import { fetchBlob } from "@/lib/api/client"
import { workspaceApi } from "@/lib/api/endpoints"
import { Button } from "@/components/ui/button"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet"
import { Spinner } from "@/components/shared/Spinner"
import {
  AuthedImage,
  downloadWithAuth,
  formatBytes,
  isImageType,
  isPreviewableText,
} from "./FileViews"

/**
 * Side panel exploring the conversation workspace — files the agent created
 * or used in its sandbox during runs (fs tools). Nothing here goes through
 * the object-storage layer; the sandbox filesystem is the source of truth.
 */
export function WorkspacePanel({
  open,
  onOpenChange,
  conversationId,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  /** Row id of the conversation — undefined for unsent drafts. */
  conversationId: string | undefined
}) {
  const { t } = useTranslation()

  const query = useQuery({
    queryKey: ["workspace", conversationId],
    queryFn: () => workspaceApi.tree(conversationId!),
    enabled: open && !!conversationId,
    staleTime: 0,
  })

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="flex w-full max-w-md flex-col gap-0 p-0 sm:max-w-md">
        <SheetHeader className="border-b px-4 py-3">
          <SheetTitle className="text-base">{t("chat.workspace.title")}</SheetTitle>
          <SheetDescription className="text-xs">
            {t("chat.workspace.subtitle")}
          </SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 py-2">
          {!conversationId ? (
            <p className="px-2 py-6 text-center text-sm text-muted-foreground">
              {t("chat.workspace.sendFirst")}
            </p>
          ) : query.isLoading ? (
            <div className="flex justify-center py-6">
              <Spinner />
            </div>
          ) : query.isError ? (
            <p className="px-2 py-6 text-center text-sm text-muted-foreground">
              {t("chat.workspace.loadFailed")}
            </p>
          ) : (query.data?.length ?? 0) === 0 ? (
            <p className="px-2 py-6 text-center text-sm text-muted-foreground">
              {t("chat.workspace.empty")}
            </p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {query.data!.map((entry) => (
                <WorkspaceRow
                  key={entry.path}
                  entry={entry}
                  conversationId={conversationId}
                />
              ))}
            </ul>
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}

function WorkspaceRow({
  entry,
  conversationId,
}: {
  entry: { path: string; is_dir: boolean; size_bytes: number | null }
  conversationId: string
}) {
  const depth = useMemo(() => entry.path.split("/").length - 1, [entry.path])

  if (entry.is_dir) {
    return (
      <li
        className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground"
        style={{ paddingLeft: `${0.5 + depth * 0.875}rem` }}
      >
        <FolderOpenIcon className="size-4 shrink-0" />
        <span className="truncate font-medium">{fileNameOf(entry.path)}</span>
      </li>
    )
  }
  return (
    <WorkspaceFileRow
      path={entry.path}
      sizeBytes={entry.size_bytes}
      depth={depth}
      conversationId={conversationId}
    />
  )
}

function WorkspaceFileRow({
  path,
  sizeBytes,
  depth,
  conversationId,
}: {
  path: string
  sizeBytes: number | null
  depth: number
  conversationId: string
}) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const [busy, setBusy] = useState(false)
  const filename = fileNameOf(path)
  const contentUrl = `/conversations/${conversationId}/workspace/content?path=${encodeURIComponent(path)}`

  return (
    <li style={{ paddingLeft: `${0.5 + depth * 0.875}rem` }}>
      <div className="group flex items-center gap-2 rounded-md px-2 py-1.5 text-sm hover:bg-muted/60">
        <FileIcon className="size-4 shrink-0" />
        <button
          type="button"
          className="flex min-w-0 flex-1 items-center gap-2 truncate text-left"
          onClick={() => setExpanded((v) => !v)}
          title={filename}
        >
          <span className="truncate">{filename}</span>
          {sizeBytes != null && (
            <span className="shrink-0 text-xs text-muted-foreground">{formatBytes(sizeBytes)}</span>
          )}
          {expanded && <ChevronDownIcon className="size-3 shrink-0 rotate-180 opacity-60" />}
        </button>
        <Button
          variant="ghost"
          size="icon"
          className="size-7 shrink-0 opacity-0 group-hover:opacity-100"
          disabled={busy}
          onClick={async () => {
            setBusy(true)
            try {
              await downloadWithAuth(contentUrl, filename)
            } catch {
              toast.error(t("chat.workspace.downloadFailed"))
            } finally {
              setBusy(false)
            }
          }}
          aria-label={t("chat.download")}
          title={t("chat.download")}
        >
          {busy ? <Spinner className="size-3.5" /> : <DownloadIcon className="size-4" />}
        </Button>
      </div>
      {expanded && <WorkspacePreview contentUrl={contentUrl} filename={filename} />}
    </li>
  )
}

/** Inline preview of one workspace file (text in a <pre>, images inline). */
function WorkspacePreview({ contentUrl, filename }: { contentUrl: string; filename: string }) {
  const [text, setText] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const textLike = isPreviewableText("", filename)
  const image = isImageType(guessMime(filename))

  useEffect(() => {
    if (!textLike) return
    let active = true
    fetchBlob(contentUrl)
      .then((b) => b.text())
      .then((body) => {
        if (active) setText(body)
      })
      .catch(() => {
        if (active) setFailed(true)
      })
    return () => {
      active = false
    }
  }, [contentUrl, textLike])

  if (textLike) {
    if (failed) {
      return (
        <p className="px-2 pb-2 text-xs text-muted-foreground" style={{ paddingLeft: "2.5rem" }}>
          —
        </p>
      )
    }
    if (text == null) {
      return (
        <div className="py-1" style={{ paddingLeft: "2.5rem" }}>
          <Spinner className="size-3.5 text-muted-foreground" />
        </div>
      )
    }
    return (
      <pre
        className="mx-2 mb-2 max-h-64 overflow-auto rounded-md border bg-muted/40 p-2 text-xs"
        style={{ marginLeft: "2.5rem" }}
      >
        {text.slice(0, 20_000)}
      </pre>
    )
  }
  if (image) {
    return (
      <div className="py-1" style={{ paddingLeft: "2.5rem" }}>
        <AuthedImage url={contentUrl} alt={filename} className="max-h-48" />
      </div>
    )
  }
  return null
}

function fileNameOf(path: string): string {
  const idx = path.lastIndexOf("/")
  return idx === -1 ? path : path.slice(idx + 1)
}

function guessMime(filename: string): string | undefined {
  const ext = filename.includes(".") ? filename.split(".").pop()!.toLowerCase() : ""
  if (["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"].includes(ext)) return `image/${ext === "jpg" ? "jpeg" : ext}`
  return undefined
}
