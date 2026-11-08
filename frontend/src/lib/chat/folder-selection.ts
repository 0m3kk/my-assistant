import { useSyncExternalStore } from "react"

/**
 * Shared "folder selected in the sidebar" state.
 *
 * It is both the destination for the next new chat and the folder shown in the
 * chat header while a draft has not been materialized yet.
 */

const KEY = "assistant_sidebar_active_folder"

function read(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

let current: string | null = read()
const listeners = new Set<() => void>()

export function getActiveFolder(): string | null {
  return current
}

export function setActiveFolder(id: string | null): void {
  if (current === id) return
  current = id
  try {
    if (id) localStorage.setItem(KEY, id)
    else localStorage.removeItem(KEY)
  } catch {
    // storage unavailable — selection stays session-only
  }
  for (const listener of listeners) listener()
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function useActiveFolder(): string | null {
  return useSyncExternalStore(subscribe, getActiveFolder, getActiveFolder)
}

/**
 * Folder a brand-new conversation was started in. It bridges the gap between
 * "first message sent" and "conversations list includes the new row": the
 * header and the sidebar can show the intended folder immediately instead of
 * falling back to Ungrouped until the next list refresh.
 */
const pendingGroups = new Map<string, string>()

export function setPendingGroup(conversationId: string, groupId: string): void {
  pendingGroups.set(conversationId, groupId)
}

export function getPendingGroup(conversationId: string): string | null {
  return pendingGroups.get(conversationId) ?? null
}

export function clearPendingGroup(conversationId: string): void {
  pendingGroups.delete(conversationId)
}
