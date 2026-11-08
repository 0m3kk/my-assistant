import { useMemo, useState } from "react"
import { useChat } from "@ai-sdk/react"
import { SendIcon } from "lucide-react"

import { createMetaChat } from "@/lib/ai/chat-manager"
import { MessageRenderer } from "@/components/chat/MessageRenderer"
import { Button } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { Spinner } from "@/components/shared/Spinner"

/** A chat bound to one built-in meta-agent. */
export function MetaChat({ agentSlug, starter }: { agentSlug: string; starter?: string }) {
  // Remount via `key` in StudioPage when the agent or starter changes.
  const chat = useMemo(() => createMetaChat(crypto.randomUUID(), agentSlug), [agentSlug])
  const { messages, sendMessage, status } = useChat({ chat })
  const [draft, setDraft] = useState(starter ?? "")
  const busy = status === "submitted" || status === "streaming"

  function send() {
    const text = draft.trim()
    if (!text || busy) return
    setDraft("")
    void sendMessage({ text })
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {messages.length === 0 ? (
          <p className="text-sm text-muted-foreground">
            Talk to <span className="font-mono">{agentSlug}</span>. It can inspect definitions, read
            feedback and executions, and create a proposal. Nothing is saved until you review and
            apply the proposal.
          </p>
        ) : (
          messages.map((message, i) => (
            <div key={message.id} className="py-3">
              <MessageRenderer
                message={message}
                streaming={busy && i === messages.length - 1}
                conversationId={chat.id}
                onApproval={() => undefined}
              />
            </div>
          ))
        )}
      </div>
      <div className="border-t p-3">
        <Textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={3}
          placeholder="Describe what you want to create or improve…"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault()
              send()
            }
          }}
        />
        <div className="mt-2 flex items-center justify-end gap-2">
          {busy && <Spinner className="size-4" />}
          <Button onClick={send} disabled={busy || !draft.trim()}>
            <SendIcon /> Send
          </Button>
        </div>
      </div>
    </div>
  )
}
