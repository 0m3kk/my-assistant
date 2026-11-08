import { Chat } from "@ai-sdk/react"
import { DefaultChatTransport, lastAssistantMessageIsCompleteWithApprovalResponses } from "ai"
import type { UIMessage } from "ai"

import { getToken } from "@/lib/api/client"

/**
 * Module-level chat instance manager.
 *
 * Each conversation owns one `Chat` instance that lives for the whole session,
 * independent of React component lifecycle. `useChat({ chat })` binds to an
 * externally-managed instance and never aborts it on unmount — so the user can
 * switch conversations (or the component can remount) without interrupting an
 * in-flight reply.
 */

const chats = new Map<string, Chat<UIMessage>>()

/** Mutable runtime context read by the transport at send time. */
export const runtimeState = {
  modelId: null as string | null,
  /** Per-request reasoning effort override (`low|medium|high`); null = model default. */
  reasoningEffort: null as string | null,
}

function authHeaders(): Record<string, string> {
  const token = getToken()
  const headers: Record<string, string> = {}
  if (token) headers.Authorization = `Bearer ${token}`
  return headers
}

const sharedTransport = new DefaultChatTransport({
  api: "/api/chat",
  headers: async () => authHeaders(),
  prepareSendMessagesRequest: async ({
    id,
    messages,
    body,
  }: {
    id: string
    messages: UIMessage[]
    body: Record<string, unknown> | undefined
  }) => ({
    body: {
      // Per-send options (e.g. `group_id` for the first message of a new
      // conversation) come from `sendMessage` and must be spread first.
      ...body,
      // The chat id IS the conversation id (client-generated, materialized
      // server-side on first send).
      conversation_id: id,
      model_id: runtimeState.modelId,
      reasoning_effort: runtimeState.reasoningEffort ?? undefined,
      messages,
    },
  }),
  // Reloaded tabs reconnect and replay the run's chunk stream.
  prepareReconnectToStreamRequest: async ({ id }: { id: string }) => ({
    api: `/api/chat/resume?conversation_id=${encodeURIComponent(id)}`,
  }),
})

export function hasChat(id: string): boolean {
  return chats.has(id)
}

/**
 * Get (or create) the Chat instance for a conversation. When the instance is
 * new and `hydrate` is provided (history fetched from the database), it is
 * seeded with those messages. An existing instance is returned untouched so a
 * live stream is never clobbered.
 */
export function getChat(id: string, hydrate?: UIMessage[]): Chat<UIMessage> {
  const existing = chats.get(id)
  if (existing) return existing
  const chat = new Chat<UIMessage>({
    id,
    messages: hydrate ?? [],
    transport: sharedTransport,
    sendAutomaticallyWhen: lastAssistantMessageIsCompleteWithApprovalResponses,
  })
  chats.set(id, chat)
  return chat
}

/**
 * A chat bound to a specific meta-agent slug (`tool-writer`, `agent-improver`,
 * …). Used by the Studio page; the conversation is created server-side on the
 * first message with that agent.
 */
export function createMetaChat(id: string, agentSlug: string): Chat<UIMessage> {
  const transport = new DefaultChatTransport({
    api: "/api/chat",
    headers: async () => authHeaders(),
    prepareSendMessagesRequest: async ({ id, messages }: { id: string; messages: UIMessage[] }) => ({
      body: {
        conversation_id: id,
        agent_slug: agentSlug,
        model_id: runtimeState.modelId,
        reasoning_effort: runtimeState.reasoningEffort ?? undefined,
        messages,
      },
    }),
  })
  return new Chat<UIMessage>({
    id,
    transport,
    sendAutomaticallyWhen: lastAssistantMessageIsCompleteWithApprovalResponses,
  })
}
