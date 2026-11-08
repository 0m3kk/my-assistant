import { useEffect, useState } from "react"
import { useTranslation } from "react-i18next"
import {
  DownloadIcon,
  FileIcon,
  FileTextIcon,
  Image as ImageIcon,
} from "lucide-react"

import { fetchBlob } from "@/lib/api/client"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/shared/Spinner"
import { cn } from "@/lib/utils"

/** Human-readable byte size. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return ""
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

/** Whether a mime type renders as an inline image preview. */
export function isImageType(contentType: string | undefined | null): boolean {
  return !!contentType && contentType.startsWith("image/")
}

/** Whether a mime type renders as a text preview. */
export function isPreviewableText(contentType: string | undefined | null, filename: string): boolean {
  if (!contentType) return false
  if (contentType.startsWith("text/") || contentType === "application/json") return true
  const ext = filename.includes(".") ? filename.split(".").pop()!.toLowerCase() : ""
  return ["json", "csv", "md", "txt", "log", "xml", "yml", "yaml", "toml", "py", "js", "ts", "rs", "go", "sql", "sh"].includes(ext)
}

/** Download an authed endpoint as a file via a temporary object URL. */
export async function downloadWithAuth(url: string, filename: string) {
  const blob = await fetchBlob(url)
  const objectUrl = URL.createObjectURL(blob)
  const a = document.createElement("a")
  a.href = objectUrl
  a.download = filename
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(objectUrl)
}

/**
 * Fetch an authed endpoint and expose it as a (revoked-on-unmount) object URL.
 * State is keyed by url, so a url change invalidates the previous result
 * without synchronous state resets.
 */
function useAuthedObjectUrl(url: string | null): { objectUrl: string | null; error: boolean } {
  const [state, setState] = useState<{
    forUrl: string
    objectUrl: string | null
    error: boolean
  } | null>(null)
  const current = state && state.forUrl === url ? state : null

  useEffect(() => {
    if (!url) return
    let active = true
    let created: string | null = null
    fetchBlob(url)
      .then((blob) => {
        if (!active) return
        created = URL.createObjectURL(blob)
        setState({ forUrl: url, objectUrl: created, error: false })
      })
      .catch(() => {
        if (active) setState({ forUrl: url, objectUrl: null, error: true })
      })
    return () => {
      active = false
      if (created) URL.revokeObjectURL(created)
    }
  }, [url])

  return current ?? { objectUrl: null, error: false }
}

/** Inline image preview fetched with the auth header. */
export function AuthedImage({
  url,
  alt,
  className,
}: {
  url: string
  alt: string
  className?: string
}) {
  const { objectUrl, error } = useAuthedObjectUrl(url)
  if (error) {
    return <div className="rounded-md border px-3 py-2 text-xs text-muted-foreground">Preview unavailable</div>
  }
  if (!objectUrl) {
    return <Spinner className="size-4 text-muted-foreground" />
  }
  return (
    <img
      src={objectUrl}
      alt={alt}
      className={cn("max-h-72 max-w-sm rounded-lg border object-contain", className)}
    />
  )
}

function FileGlyph({ contentType }: { contentType?: string | null }) {
  if (isImageType(contentType)) return <ImageIcon className="size-4 shrink-0" />
  if (isPreviewableText(contentType, "")) return <FileTextIcon className="size-4 shrink-0" />
  return <FileIcon className="size-4 shrink-0" />
}

/** File metadata card: icon, name, size, authed download. */
export function FileCard({
  filename,
  contentType,
  sizeBytes,
  url,
  className,
}: {
  filename: string
  contentType?: string | null
  sizeBytes?: number | null
  url: string
  className?: string
}) {
  const { t } = useTranslation()
  const [busy, setBusy] = useState(false)

  const onDownload = async () => {
    setBusy(true)
    try {
      await downloadWithAuth(url, filename)
    } catch {
      // fetchBlob surfaces failures via toast at call sites when needed;
      // here we just reset.
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className={cn(
        "flex w-fit max-w-sm items-center gap-3 rounded-xl border px-3 py-2 text-sm",
        className,
      )}
    >
      <FileGlyph contentType={contentType} />
      <div className="min-w-0">
        <p className="truncate font-medium">{filename}</p>
        {sizeBytes != null && (
          <p className="text-xs text-muted-foreground">{formatBytes(sizeBytes)}</p>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon"
        className="ml-1 size-7 shrink-0"
        onClick={() => void onDownload()}
        disabled={busy}
        aria-label={t("chat.download")}
        title={t("chat.download")}
      >
        {busy ? <Spinner className="size-3.5" /> : <DownloadIcon className="size-4" />}
      </Button>
    </div>
  )
}

/** Card with an image preview on top (used for image attachments/artifacts). */
export function ImageFileCard({
  filename,
  contentType,
  url,
}: {
  filename: string
  contentType?: string | null
  url: string
}) {
  return (
    <div className="flex w-fit max-w-sm flex-col items-stretch gap-2">
      <AuthedImage url={url} alt={filename} />
      <FileCard filename={filename} contentType={contentType} url={url} />
    </div>
  )
}

/** Artifact card rendered from a `save_artifact` tool output. */
export function ArtifactCard({ artifact }: { artifact: ArtifactData }) {
  return isImageType(artifact.content_type) ? (
    <ImageFileCard
      filename={artifact.filename}
      contentType={artifact.content_type}
      url={artifact.url}
    />
  ) : (
    <FileCard
      filename={artifact.filename}
      contentType={artifact.content_type}
      sizeBytes={artifact.size_bytes}
      url={artifact.url}
    />
  )
}

/** Shape of the `artifact` object embedded in save_artifact tool output. */
export interface ArtifactData {
  id: string
  filename: string
  content_type: string
  size_bytes: number
  url: string
}
