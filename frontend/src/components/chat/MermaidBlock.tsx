import { useEffect, useMemo, useState } from "react"
import { useTranslation } from "react-i18next"
import { Maximize2Icon, MinusIcon, PlusIcon, RotateCcwIcon, XIcon } from "lucide-react"

import { usePreferences } from "@/lib/theme/PreferencesProvider"
import { CodeBlock, CodeBlockContent } from "@/components/ui/code-block"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog"

// Mermaid uses the supplied id as the SVG root id and cleans up by id, so ids
// MUST be unique across the whole page — otherwise rendering a later diagram
// removes an earlier one from the DOM. A UUID suffix keeps every id the same
// length, so one id can never be a prefix of another. Renders are also
// serialized because mermaid keeps global (module-level) state.
let renderQueue: Promise<unknown> = Promise.resolve()

function enqueueRender<T>(fn: () => Promise<T>): Promise<T> {
  const run = renderQueue.catch(() => undefined).then(fn)
  renderQueue = run.catch(() => undefined)
  return run
}

const clampZoom = (z: number) => Math.min(4, Math.max(0.25, Math.round(z * 100) / 100))

/** Natural size of a rendered diagram, from its `viewBox`. */
function svgSize(svg: string): { width: number; height: number } | null {
  const viewBox = svg.match(/viewBox\s*=\s*["']([^"']+)["']/i)
  if (viewBox) {
    const parts = viewBox[1].trim().split(/[\s,]+/).map(Number)
    const width = parts[2]
    const height = parts[3]
    if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) {
      return { width, height }
    }
  }
  // Fallback: explicit numeric width/height attributes (never "100%").
  const width = Number(svg.match(/\swidth\s*=\s*["']([\d.]+)(?:px)?["']/i)?.[1])
  const height = Number(svg.match(/\sheight\s*=\s*["']([\d.]+)(?:px)?["']/i)?.[1])
  if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) {
    return { width, height }
  }
  return null
}

/**
 * Make the root SVG responsive by rewriting its opening tag: drop mermaid's
 * width/height attributes and inline `max-width` (from `useMaxWidth`), then
 * force `width: min(100%, <natural * 1.5>)` so it centers in its container,
 * shrinks when too wide, and only moderately enlarges small diagrams (a plain
 * 100% width stretches tiny charts across the whole chat pane). Inline styles
 * are used so no CSS specificity fight with the SVG's own attributes.
 */
function responsiveSvg(svg: string): string {
  const start = svg.indexOf("<svg")
  const end = start === -1 ? -1 : svg.indexOf(">", start)
  if (start === -1 || end === -1) return svg
  const size = svgSize(svg)
  const maxWidth = size ? `${Math.round(size.width * 1.1)}px` : "100%"
  let tag = svg.slice(start, end)
  tag = tag.replace(/\sstyle=(["'])[^"']*\1/i, "")
  tag = tag.replace(/\s(?:width|height)=(["'])[^"']*\1/gi, "")
  tag += ` style="display:block;width:min(100%, ${maxWidth});height:auto;max-width:none;margin-inline:auto;"`
  return svg.slice(0, start) + tag + svg.slice(end)
}

/** Effective dark flag: explicit preference, or live OS mode when "system". */
function useIsDark(): boolean {
  const { theme } = usePreferences()
  const [mediaDark, setMediaDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches,
  )

  // Re-sync OS media state when the preference switches to/from "system".
  // (Reset-state-when-prop-changes during render — the documented pattern;
  // avoids synchronous setState inside effects.)
  const [prevTheme, setPrevTheme] = useState(theme)
  if (prevTheme !== theme) {
    setPrevTheme(theme)
    setMediaDark(window.matchMedia("(prefers-color-scheme: dark)").matches)
  }

  useEffect(() => {
    if (theme !== "system") return
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const onChange = (e: MediaQueryListEvent) => setMediaDark(e.matches)
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [theme])

  return theme === "dark" || (theme === "system" && mediaDark)
}

/**
 * Renderer for ```mermaid fences: lazily loads mermaid (code-split chunk),
 * renders to an SVG, and falls back to the raw code block on parse failure.
 * Re-renders when the theme flips between light and dark. State is keyed by
 * (code, theme) so a new input invalidates the previous result without
 * synchronous state resets.
 *
 * Clicking a rendered diagram opens a modal where it can be zoomed and
 * scrolled (large/wide diagrams are unreadable inline).
 */
export function MermaidBlock({ code }: { code: string }) {
  const { t } = useTranslation()
  const isDark = useIsDark()
  const [state, setState] = useState<{
    id: string
    forCode: string
    forDark: boolean
    svg: string | null
    failed: boolean
  } | null>(null)
  const current = state && state.forCode === code && state.forDark === isDark ? state : null
  const [open, setOpen] = useState(false)
  const [zoom, setZoom] = useState(1)

  // The dialog renders a second copy of the SVG. Rewrite the root render id so
  // the two copies don't collide — mermaid prefixes every internal id with it,
  // so replacing that prefix consistently keeps references intact.
  const popupSvg = useMemo(
    () => (current?.svg ? current.svg.split(current.id).join(`${current.id}-popup`) : null),
    [current],
  )
  const popupSize = useMemo(() => (current?.svg ? svgSize(current.svg) : null), [current])

  useEffect(() => {
    if (current) return
    let active = true
    // Debounce while streaming: the fence content changes on every delta, and
    // each intermediate version would just fail to parse (or waste renders).
    const timer = setTimeout(async () => {
      try {
        const rendered = await enqueueRender(async () => {
          const { default: mermaid } = await import("mermaid")
          mermaid.initialize({
            startOnLoad: false,
            securityLevel: "strict",
            // Failed parses throw (we fall back to source) but must not inject
            // their error SVGs into the page.
            suppressErrorRendering: true,
            theme: isDark ? "dark" : "default",
            fontFamily: "var(--font-mono)",
          })
          const id = `mermaid-${crypto.randomUUID()}`
          const { svg } = await mermaid.render(id, code)
          // Force a responsive root size (see responsiveSvg). The popup uses
          // its own explicit dimensions on top of this.
          return { id, svg: responsiveSvg(svg) }
        })
        if (active) {
          setState({
            id: rendered.id,
            forCode: code,
            forDark: isDark,
            svg: rendered.svg,
            failed: false,
          })
        }
      } catch {
        if (active) setState({ id: "", forCode: code, forDark: isDark, svg: null, failed: true })
      }
    }, 500)
    return () => {
      active = false
      clearTimeout(timer)
    }
  }, [code, isDark, current])

  if (current?.failed) {
    // Diagram could not parse — show the source as a normal code block.
    return (
      <CodeBlock className="my-3 rounded-md border bg-muted/30">
        <CodeBlockContent className="px-4 py-3">{code}</CodeBlockContent>
      </CodeBlock>
    )
  }

  return (
    <>
      {current?.svg == null ? (
        <div
          data-slot="mermaid-block"
          className="my-3 flex max-w-full justify-center overflow-x-auto rounded-md border bg-muted/40 p-3"
        >
          <span className="py-2 text-xs text-muted-foreground">{t("chat.mermaid.rendering")}</span>
        </div>
      ) : (
        <div
          data-slot="mermaid-block"
          role="button"
          tabIndex={0}
          aria-label={t("chat.mermaid.expand")}
          title={t("chat.mermaid.expand")}
          onClick={() => {
            setZoom(1)
            setOpen(true)
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault()
              setZoom(1)
              setOpen(true)
            }
          }}
          className="group/mermaid relative my-3 flex max-w-full cursor-zoom-in justify-center overflow-x-auto rounded-md border bg-muted/40 p-3 outline-none transition-colors hover:border-primary/40 focus-visible:ring-[3px] focus-visible:ring-ring/50"
        >
          <div
            className="w-full [&>svg]:block [&>svg]:h-auto [&>svg]:w-full [&>svg]:max-w-none"
            dangerouslySetInnerHTML={{ __html: current.svg }}
          />
          <span className="pointer-events-none absolute top-1.5 right-1.5 rounded-md bg-background/80 p-1.5 opacity-0 shadow-sm transition-opacity group-hover/mermaid:opacity-100">
            <Maximize2Icon className="size-3.5" />
          </span>
        </div>
      )}

      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent
          showCloseButton={false}
          aria-describedby={undefined}
          className="flex h-[85vh] w-[90vw] max-w-[95vw] flex-col gap-3 sm:max-w-[95vw]"
        >
          <DialogHeader className="flex-row items-center justify-between gap-2 space-y-0">
            <DialogTitle className="text-base">{t("chat.mermaid.title")}</DialogTitle>
            <div className="flex items-center gap-1">
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => setZoom((z) => clampZoom(z - 0.25))}
                title={t("chat.mermaid.zoomOut")}
                aria-label={t("chat.mermaid.zoomOut")}
              >
                <MinusIcon />
              </Button>
              <span className="w-12 text-center text-xs text-muted-foreground tabular-nums">
                {Math.round(zoom * 100)}%
              </span>
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => setZoom((z) => clampZoom(z + 0.25))}
                title={t("chat.mermaid.zoomIn")}
                aria-label={t("chat.mermaid.zoomIn")}
              >
                <PlusIcon />
              </Button>
              <Button
                variant="outline"
                size="icon-sm"
                onClick={() => setZoom(1)}
                title={t("chat.mermaid.reset")}
                aria-label={t("chat.mermaid.reset")}
              >
                <RotateCcwIcon />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => setOpen(false)}
                title={t("chat.mermaid.close")}
                aria-label={t("chat.mermaid.close")}
              >
                <XIcon />
              </Button>
            </div>
          </DialogHeader>
          <div className="min-h-0 flex-1 overflow-auto rounded-md border bg-muted/30 p-4">
            {popupSvg && (
              // The mermaid SVG has an inline `max-width` (useMaxWidth), which
              // defeats a CSS `zoom` on its container. Instead, a spacer gets
              // the scaled dimensions (so the scroll area is correct) and the
              // diagram is `transform`-scaled inside it. `m-auto` centers the
              // spacer while it fits and collapses to 0 once it overflows.
              <div className="flex min-h-full min-w-full">
                {popupSize ? (
                  <div
                    className="m-auto shrink-0"
                    style={{ width: popupSize.width * zoom, height: popupSize.height * zoom }}
                  >
                    <div
                      style={{
                        width: popupSize.width,
                        height: popupSize.height,
                        transform: `scale(${zoom})`,
                        transformOrigin: "top left",
                      }}
                      className="[&>svg]:block [&>svg]:h-full [&>svg]:max-w-none [&>svg]:w-full"
                      dangerouslySetInnerHTML={{ __html: popupSvg }}
                    />
                  </div>
                ) : (
                  <div
                    className="m-auto [&>svg]:h-auto [&>svg]:max-w-none"
                    dangerouslySetInnerHTML={{ __html: popupSvg }}
                  />
                )}
              </div>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </>
  )
}
