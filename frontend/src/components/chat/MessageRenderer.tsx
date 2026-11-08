import { memo, useEffect, useRef, useState } from "react"
import ReactMarkdown, { type Components, type ExtraProps } from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"

import { isToolUIPart, getToolName } from "ai"
import type { FileUIPart, UIMessage } from "ai"
import { JSONUIProvider, Renderer, useJsonRenderMessage } from "@json-render/react"
import { useTranslation } from "react-i18next"
import { BrainIcon, ChevronDownIcon } from "lucide-react"

import { cn } from "@/lib/utils"
import type { Approval, FeedbackRow } from "@/lib/types"
import { messageRunId, messageText } from "@/lib/types"
import { highlightToHtml } from "@/lib/ai/markdown-highlight"
import { Message, MessageContent } from "@/components/ui/message"
import { Bubble, BubbleContent } from "@/components/ui/bubble"
import {
  CodeBlock,
  CodeBlockActions,
  CodeBlockContent,
  CodeBlockCopyButton,
  CodeBlockHeader,
  CodeBlockTitle,
} from "@/components/ui/code-block"
import { CopyButton } from "@/components/shared/CopyButton"
import { Spinner } from "@/components/shared/Spinner"
import { FileCard, ImageFileCard, isImageType } from "./FileViews"
import { MermaidBlock } from "./MermaidBlock"
import { ToolPartLine, type ToolPartData } from "./ToolPartLine"
import { ApprovalCard } from "./ApprovalCard"
import { FeedbackControl, findAgentFeedback } from "./FeedbackControl"
import { dataPartRenderer, toolOutputRenderer } from "./registries"
import { widgetRegistry } from "./widgets/registry"

interface MessageRendererProps {
  message: UIMessage
  streaming: boolean
  conversationId: string
  feedback?: FeedbackRow[]
  /** Approval ledger rows for this conversation (history badges). */
  approvals?: Approval[]
  /** Called when the user approves/denies — wires into addToolApprovalResponse. */
  onApproval: (approvalId: string, approved: boolean) => void
}

export const MessageRenderer = memo(function MessageRenderer({
  message,
  streaming,
  conversationId,
  feedback,
  approvals,
  onApproval,
}: MessageRendererProps) {
  // json-render: fold the message's `data-spec` parts into a UI spec.
  // Called unconditionally (before any early return) to keep hook order stable.
  const { spec, hasSpec } = useJsonRenderMessage(message.parts)
  const specIndex = message.parts.findIndex((p) => p.type === "data-spec")

  if (message.role === "user") {
    const text = message.parts
      .filter((p): p is Extract<UIMessage["parts"][number], { type: "text" }> => p.type === "text")
      .map((p) => p.text)
      .join("\n")
    const files = message.parts.filter((p): p is FileUIPart => p.type === "file")
    return (
      <Message align="end">
        <MessageContent className="flex flex-col items-end">
          {files.map((fp, i) => (
            <FileCardFromPart key={i} part={fp} />
          ))}
          {text && (
            <Bubble align="end">
              <BubbleContent>{text}</BubbleContent>
            </Bubble>
          )}
        </MessageContent>
      </Message>
    )
  }

  const runId = messageRunId(message)

  // Messages that only carry transport metadata (data-run/step-start, …) have
  // nothing to show; rendering them would leave an empty block that still
  // reveals the feedback row on hover.
  const hasVisibleContent = message.parts.some((p) => {
    if (p.type === "text" || p.type === "reasoning") return p.text.trim().length > 0
    if (p.type === "file") return true
    if (p.type === "step-start") return false
    if (isToolUIPart(p)) return true
    if (p.type.startsWith("data-")) {
      const kind = p.type.slice("data-".length)
      if (kind === "spec") return hasSpec
      return kind !== "conversation" && kind !== "run" && kind !== "usage"
    }
    return false
  })
  if (!hasVisibleContent) return null

  // Answer text only — `messageText` joins `text` parts and skips reasoning,
  // tool calls and other data parts.
  const copyText = messageText(message).trim()

  return (
    <Message align="start">
      <MessageContent>
        <div className="flex max-w-full flex-col gap-2">
          {message.parts.map((part, index) => (
            <PartRenderer
              key={index}
              part={part}
              streaming={streaming && index === message.parts.length - 1}
              onApproval={onApproval}
              conversationId={conversationId}
              feedback={feedback}
              approvals={approvals}
              spec={spec}
              hasSpec={hasSpec}
              isSpecRoot={index === specIndex}
              specLoading={streaming}
            />
          ))}
        </div>
        {!streaming && (copyText || runId) && (
          <div className="flex items-center gap-1">
            {copyText && <CopyButton value={copyText} />}
            {runId && (
              <FeedbackControl
                conversationId={conversationId}
                scope="agent_run"
                runId={runId}
                initial={findAgentFeedback(feedback, runId)}
              />
            )}
          </div>
        )}
      </MessageContent>
    </Message>
  )
})

type UIPart = UIMessage["parts"][number]

/** Attachment card for a `file` part (image preview or plain file card). */
function FileCardFromPart({ part }: { part: FileUIPart }) {
  const filename = part.filename ?? "file"
  return isImageType(part.mediaType) ? (
    <ImageFileCard filename={filename} contentType={part.mediaType} url={part.url} />
  ) : (
    <FileCard filename={filename} contentType={part.mediaType} url={part.url} />
  )
}

function PartRenderer({
  part,
  streaming,
  onApproval,
  conversationId,
  feedback,
  approvals,
  spec,
  hasSpec,
  isSpecRoot,
  specLoading,
}: {
  part: UIPart
  streaming: boolean
  onApproval: (approvalId: string, approved: boolean) => void
  conversationId: string
  feedback?: FeedbackRow[]
  approvals?: Approval[]
  /** json-render spec folded from this message's data-spec parts. */
  spec: ReturnType<typeof useJsonRenderMessage>["spec"]
  hasSpec: boolean
  /** True for the first data-spec part — the one position the UI renders at. */
  isSpecRoot: boolean
  /** While the message streams, the renderer shows its loading state. */
  specLoading: boolean
}) {
  if (part.type === "text") {
    if (!part.text) return null
    return (
      <MarkdownBlock>
        {part.text}
      </MarkdownBlock>
    )
  }

  if (part.type === "reasoning") {
    const text = (part as { text?: string }).text ?? ""
    if (!text) return null
    // `streaming` is true only while the message streams AND this part is the
    // last one — i.e. the model is reasoning right now.
    return <ReasoningBlock text={text} inProgress={streaming} />
  }

  if (part.type === "file") {
    return <FileCardFromPart part={part as FileUIPart} />
  }

  if (part.type === "step-start") {
    return null
  }

  if (isToolUIPart(part)) {
    const toolPart = part as unknown as ToolPartData & { approval?: { id?: string; approved?: boolean } }
    const data: ToolPartData = {
      toolCallId: toolPart.toolCallId,
      toolName: getToolName(part),
      state: (toolPart.state ?? "input-available") as ToolPartData["state"],
      input: toolPart.input,
      output: toolPart.output,
      errorText: toolPart.errorText,
      approved: toolPart.approval?.approved,
    }

    // A pending approval is a question for the user, not a step the agent took.
    if (data.state === "approval-requested") {
      const approvalId = toolPart.approval?.id ?? ""
      return (
        <ApprovalCard
          approvalId={approvalId}
          conversationId={conversationId}
          toolName={data.toolName}
          input={data.input}
          onDecided={onApproval}
        />
      )
    }

    // Custom output renderer (registry keyed by tool name).
    if (data.state === "output-available") {
      const renderer = toolOutputRenderer(data.toolName)
      if (renderer) {
        return <>{renderer(data.output, data.input)}</>
      }
    }

    return (
      <ToolPartLine
        part={data}
        conversationId={conversationId}
        feedback={feedback}
        approval={approvals?.find((a) => a.tool_call_id === data.toolCallId) ?? null}
      />
    )
  }

  if (part.type.startsWith("data-")) {
    const kind = part.type.slice("data-".length)
    if (kind === "conversation" || kind === "run") return null // transport metadata
    // Usage feeds the composer's context meter; nothing to render inline.
    if (kind === "usage") return null
    if (kind === "spec") {
      // json-render UI: folded from every data-spec part in the message and
      // rendered once, at the first part's position (later ones are fold input).
      if (!isSpecRoot || !hasSpec || !spec) return null
      return (
        <JSONUIProvider registry={widgetRegistry}>
          <Renderer spec={spec} registry={widgetRegistry} loading={specLoading} />
        </JSONUIProvider>
      )
    }
    const renderer = dataPartRenderer(kind)
    if (renderer) {
      const data = (part as { data?: unknown }).data
      return <>{renderer(data)}</>
    }
    // Unknown data part — transparent JSON fallback.
    return (
      <details className="my-1 max-w-lg rounded-md border px-3 py-2 text-xs">
        <summary className="cursor-pointer text-muted-foreground">data:{kind}</summary>
        <pre className="mt-2 overflow-auto">{JSON.stringify(part, null, 2)}</pre>
      </details>
    )
  }

  return null
}

/**
 * Collapsible reasoning ("thinking") display.
 *
 * - Collapsed while the model is reasoning: keep a small tail preview (last
 *   ~3 lines) so the run stays visible, like a streaming terminal.
 * - Expanded (click): show the full reasoning text.
 * - Collapsed after reasoning finished: collapse everything to the header.
 */
function ReasoningBlock({ text, inProgress }: { text: string; inProgress: boolean }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const contentRef = useRef<HTMLDivElement | null>(null)
  // Follow the newest reasoning text while expanded, until the user scrolls up.
  const stickToBottomRef = useRef(true)
  const showPreview = !expanded && inProgress
  const showFull = expanded

  useEffect(() => {
    if (!showFull) return
    const el = contentRef.current
    if (!el || !stickToBottomRef.current) return
    el.scrollTop = el.scrollHeight
  }, [text, showFull])

  const handleContentScroll = () => {
    const el = contentRef.current
    if (!el) return
    stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < 16
  }

  return (
    <div className="my-1 max-w-3xl overflow-hidden rounded-md border border-violet-500/20 text-sm text-violet-700/75 dark:text-violet-300/65">
      {/* Distinct violet identity for the reasoning block (tool runs are emerald);
          content text uses a softer tone of the same hue. */}
      <button
        type="button"
        onClick={() => {
          // Expanding always starts pinned to the newest text.
          if (!expanded) stickToBottomRef.current = true
          setExpanded((v) => !v)
        }}
        aria-expanded={expanded}
        className="flex w-full items-center gap-2 px-3 py-2 text-left text-violet-600 outline-none transition-colors hover:text-violet-700 dark:text-violet-400/85 dark:hover:text-violet-300/90"
      >
        {inProgress ? (
          <Spinner className="size-3.5 shrink-0 text-violet-600 dark:text-violet-400/85" />
        ) : (
          <BrainIcon className="size-3.5 shrink-0" />
        )}
        <span className={cn("min-w-0 flex-1 truncate", inProgress && "animate-pulse")}>
          {inProgress ? t("chat.reasoning.thinking") : t("chat.reasoning.thought")}
        </span>
        <ChevronDownIcon
          className={cn("size-3.5 shrink-0 opacity-60 transition-transform", expanded && "rotate-180")}
        />
      </button>
      {(showFull || showPreview) && (
        <div
          ref={contentRef}
          onScroll={handleContentScroll}
          className={cn(
            "px-3 pb-2",
            // Tail preview: pin the content to the bottom of a fixed-height,
            // overflow-hidden box so the newest lines stay visible, fading the
            // top edge out.
            showPreview &&
              "flex max-h-[3.75rem] flex-col justify-end overflow-hidden [mask-image:linear-gradient(to_bottom,transparent,black_26px)]",
            showFull && "max-h-80 overflow-y-auto",
          )}
        >
          <p className="text-xs break-words leading-relaxed whitespace-pre-wrap">{text}</p>
        </div>
      )}
    </div>
  )
}

export function MarkdownBlock({ children }: { children: string }) {
  return (
    <div
      className={cn(
        "prose max-w-none text-base",
        // Typography's palette defaults to gray-*; use the app's tokens so
        // chat text has the same contrast as the UI. Body copy is slightly
        // softened (85%) — full foreground at this size reads as glaring.
        // The vars flip with the theme, so no `prose-invert` is needed.
        "[--tw-prose-body:color-mix(in_oklab,var(--color-foreground)_85%,transparent)]",
        "[--tw-prose-lead:color-mix(in_oklab,var(--color-foreground)_85%,transparent)]",
        // Dark mode: near-white on near-black is harsher at body size, so
        // dim body/heading copy further. Keep bold/link slightly brighter
        // than body so hierarchy survives the lower contrast.
        "dark:[--tw-prose-body:color-mix(in_oklab,var(--color-foreground)_72%,transparent)]",
        "dark:[--tw-prose-lead:color-mix(in_oklab,var(--color-foreground)_72%,transparent)]",
        "dark:[--tw-prose-headings:color-mix(in_oklab,var(--color-foreground)_86%,transparent)]",
        "dark:[--tw-prose-bold:color-mix(in_oklab,var(--color-foreground)_86%,transparent)]",
        "dark:[--tw-prose-links:color-mix(in_oklab,var(--color-foreground)_86%,transparent)]",
        "dark:[--tw-prose-quotes:color-mix(in_oklab,var(--color-foreground)_78%,transparent)]",
        "dark:[--tw-prose-code:color-mix(in_oklab,var(--color-foreground)_82%,transparent)]",
        "dark:[--tw-prose-pre-code:color-mix(in_oklab,var(--color-foreground)_82%,transparent)]",
        "[--tw-prose-links:var(--color-foreground)]",
        "[--tw-prose-bold:var(--color-foreground)]",
        "[--tw-prose-counters:var(--color-muted-foreground)]",
        "[--tw-prose-bullets:var(--color-muted-foreground)]",
        "[--tw-prose-hr:var(--color-border)]",
        "[--tw-prose-quotes:var(--color-foreground)]",
        "[--tw-prose-quote-borders:var(--color-border)]",
        "[--tw-prose-captions:var(--color-muted-foreground)]",
        "[--tw-prose-code:var(--color-foreground)]",
        "[--tw-prose-pre-code:var(--color-foreground)]",
        "[--tw-prose-pre-bg:transparent]",
        "[--tw-prose-th-borders:var(--color-border)]",
        "[--tw-prose-td-borders:var(--color-border)]",
        // Roomier text: taller line-height and clearer paragraph/list rhythm.
        // Selective overrides — code blocks keep their compact line-height so
        // ASCII layouts stay aligned.
        "[&_p]:my-3 [&_p]:leading-relaxed",
        "[&_ul]:my-3 [&_ol]:my-3 [&_li]:my-1.5 [&_li]:leading-relaxed",
        "[&_h1]:mt-5 [&_h2]:mt-5 [&_h3]:mt-5 [&_h4]:mt-4 [&_h1]:mb-2 [&_h2]:mb-2 [&_h3]:mb-2 [&_h4]:mb-2",
        // Code blocks are fully custom (CodeBlock + shiki) — strip the prose
        // pre/code decoration so nothing double-styles them.
        "prose-pre:bg-transparent prose-pre:p-0 prose-pre:m-0 prose-pre:rounded-none",
        "prose-code:before:content-none prose-code:after:content-none",
        // Lists: keep markers hanging comfortably inside the box — some zoom /
        // fractional-pixel combinations can clip the digits at the left edge.
        "[&_ol]:list-outside [&_ul]:list-outside [&_ol]:ps-7 [&_ul]:ps-6",
        "[&_ol]:marker:text-muted-foreground [&_ul]:marker:text-muted-foreground",
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm, remarkMath]} rehypePlugins={[rehypeKatex]} components={markdownComponents}>
        {children}
      </ReactMarkdown>
    </div>
  )
}

// ---------- Markdown component overrides ----------

const markdownComponents: Components = {
  // Fenced code / mermaid (also adds language label + copy button).
  pre: MarkdownPre,
  // Inline code chip (fenced code never reaches this — pre is replaced).
  code: ({ node: _node, className, children, ...props }) => {
    if (className?.includes("language-")) {
      return (
        <code className={className} {...props}>
          {children}
        </code>
      )
    }
    return (
      <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-[0.85em]" {...props}>
        {children}
      </code>
    )
  },
  a: ({ children, ...props }) => (
    <a {...props} target="_blank" rel="noreferrer" className="underline underline-offset-4">
      {children}
    </a>
  ),
  // Horizontal scroll wrapper with a little breathing room from the border —
  // wide tables must not stretch the bubble either.
  table: ({ children }) => (
    <div className="my-3 w-full overflow-x-auto rounded-md border p-1.5">
      <table>{children}</table>
    </div>
  ),
  img: ({ node: _node, ...props }) => (
    // eslint-disable-next-line jsx-a11y/alt-text
    <img {...props} loading="lazy" className="my-3 max-w-full rounded-md border" />
  ),
}

/** Extract the raw text of a hast subtree (used for code + copy). */
function hastText(node: { type: string; value?: string; children?: unknown[] }): string {
  if (node.type === "text") return node.value ?? ""
  if (Array.isArray(node.children)) return node.children.map((c) => hastText(c as never)).join("")
  return ""
}

/** Minimal structural shape of a hast code element (avoids hast type import). */
type PreProps = React.ComponentProps<"pre"> & ExtraProps

/**
 * Fenced code block: mermaid fences render as diagrams; everything else goes
 * through shiki (async, cached) inside a CodeBlock with a language label and
 * copy button. Until shiki resolves, the raw text shows — cached blocks are
 * effectively instant, only the actively-streaming block flashes plain.
 */
function MarkdownPre({ node, children }: PreProps) {
  const hastChildren = (node?.children ?? []) as Array<{
    type: string
    tagName?: string
    properties?: Record<string, unknown>
    children?: unknown[]
  }>
  const codeEl = hastChildren.find((c) => c.type === "element" && c.tagName === "code")
  if (!node || !codeEl) {
    // No hast node (defensive) — render the default mapping result.
    return (
      <CodeBlock className="my-3 rounded-md border bg-muted/30">
        <CodeBlockContent className="px-4 py-3">{children}</CodeBlockContent>
      </CodeBlock>
    )
  }
  const classNames = Array.isArray(codeEl.properties?.className)
    ? (codeEl.properties.className as string[])
    : []
  const lang = classNames.find((c) => c.startsWith("language-"))?.slice("language-".length) ?? ""
  const raw = hastText(codeEl)

  if (lang === "mermaid") {
    return <MermaidBlock code={raw} />
  }

  return (
    <CodeBlock className="my-3 rounded-md border bg-muted/30">
      <CodeBlockHeader className="border-b bg-muted/50 px-3 py-1">
        <CodeBlockTitle className="text-xs text-muted-foreground">{lang || "text"}</CodeBlockTitle>
        <CodeBlockActions>
          <CodeBlockCopyButton value={raw} />
        </CodeBlockActions>
      </CodeBlockHeader>
      <HighlightedCode code={raw} lang={lang} />
    </CodeBlock>
  )
}

/** Async shiki render with a plain-text fallback while loading. */
function HighlightedCode({ code, lang }: { code: string; lang: string }) {
  const [html, setHtml] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    highlightToHtml(code, lang)
      .then((result) => {
        if (active) setHtml(result)
      })
      .catch(() => undefined)
    return () => {
      active = false
    }
  }, [code, lang])

  if (html == null) {
    return <CodeBlockContent className="px-4 py-3">{code}</CodeBlockContent>
  }
  return <div dangerouslySetInnerHTML={{ __html: html }} />
}
