import type { UIMessage } from "ai"

// ---------- API shapes (mirror backend DTOs) ----------

export interface PublicUser {
  id: string
  email: string
  name: string
  avatar_url: string | null
  role: string
}

export interface AuthResponse {
  token: string
  user: PublicUser
}

export interface Meta {
  registration_enabled: boolean
}

export interface UserPreferences {
  user_id: string
  theme: "system" | "light" | "dark"
  font_scale: "sm" | "md" | "lg" | "xl"
  locale: string
  /** Model used to generate conversation titles (null = global default). */
  title_model_id: string | null
}

export interface ConversationSummary {
  id: string
  agent_id: string
  title: string
  status: string
  group_id: string | null
  last_message_at: string | null
  created_at: string
}

/** One-level, user-defined conversation folder. */
export interface ConversationGroup {
  id: string
  name: string
  position: number
}

export interface StoredMessage {
  id: string
  role: "user" | "assistant"
  parts: UIMessage["parts"]
}

/** Token accounting for one assistant turn (mirror of backend `data-usage`). */
export interface UsageSnapshot {
  promptTokens: number | null
  completionTokens: number | null
  /** Prompt tokens of the final provider call — the context fill level. */
  contextTokens: number | null
  /** Model context window, when the model row declares one. */
  contextWindow: number | null
}

/** Uploaded attachment / generated artifact (mirror of backend FileDto). */
export interface FileDto {
  id: string
  origin: "attachment" | "artifact"
  filename: string
  content_type: string
  size_bytes: number
  checksum_sha256: string | null
  /** Authed download route, e.g. /api/files/{id}/content. */
  url: string
  created_at: string
}

/** One entry of the conversation workspace tree (agent sandbox files). */
export interface WorkspaceEntry {
  path: string
  is_dir: boolean
  size_bytes: number | null
  modified_at: string | null
}

export interface Provider {
  id: string
  name: string
  preset: string
  base_url: string
  secret_id: string | null
  status: string
}

/** Reasoning effort levels sent as `reasoning_effort` to OpenAI-compatible APIs. */
export type ReasoningEffort = "low" | "medium" | "high" | "xhigh"

/** Canonical level list (order = ascending effort). */
export const REASONING_EFFORT_LEVELS: ReasoningEffort[] = ["low", "medium", "high", "xhigh"]

export interface Model {
  id: string
  provider_id: string
  api_name: string
  display_name: string
  context_window: number | null
  capabilities: { tools?: boolean; vision?: boolean; reasoning?: boolean }
  reasoning_effort: ReasoningEffort | null
  is_default: boolean
  status: string
}

export interface ModelOption {
  id: string
  provider_id: string
  provider_name: string
  display_name: string
  api_name: string
  context_window: number | null
  is_default: boolean
  capabilities: { tools?: boolean; vision?: boolean; reasoning?: boolean }
  reasoning_effort: ReasoningEffort | null
}

/** One entry from `GET /admin/providers/{id}/models/discover`. */
export interface DiscoveredModel {
  api_name: string
  display_name: string
  context_window: number | null
  /** True when a model with this api_name is already configured. */
  configured: boolean
}

/** One entry from `GET /agents` (Agent Selector). */
export interface AgentOption {
  id: string
  name: string
  slug: string
  description: string
  default_model_id: string | null
  is_builtin: boolean
}

export interface Agent {
  id: string
  name: string
  slug: string
  description: string
  system_prompt: string
  default_model_id: string | null
  temperature: number | null
  max_tokens: number | null
  max_tool_calls: number
  kind: "chat" | "meta"
  is_builtin: boolean
  revision: number
  status: string
  group_id: string | null
  created_by: string | null
}

/** One-level, admin-defined agent folder. */
export interface AgentGroup {
  id: string
  name: string
  position: number
}

export type ToolKind = "builtin" | "http" | "file" | "wasm" | "native" | "mcp"

export interface ToolRow {
  id: string
  name: string
  display_name: string
  description: string
  kind: ToolKind
  source_ref: string
  config: Record<string, unknown>
  parameters_schema: Record<string, unknown>
  requires_approval: boolean
  is_meta: boolean
  is_builtin: boolean
  revision: number
  status: string
  group_id: string | null
  created_by: string | null
}

/** One-level, admin-defined tool folder. */
export interface ToolGroup {
  id: string
  name: string
  position: number
}

export interface AgentToolAssignment {
  tool_id: string
  name: string
  display_name: string
  kind: string
  requires_approval: boolean
  approval_override: boolean
  config: Record<string, unknown> | null
}

export interface SecretPublic {
  id: string
  name: string
  description: string
  masked: string
  version: number
  last_used_at: string | null
  created_at: string
  updated_at: string
}

export interface ProviderPreset {
  id: string
  label: string
  base_url: string
  hint: string
}

export interface Approval {
  id: string
  conversation_id: string
  tool_call_id: string
  tool_name: string
  input: unknown
  reason: "requires_approval" | "out_of_root"
  requested_resource: string | null
  status: "pending" | "approved" | "denied"
  decision_mode: "once" | "propose_update" | null
  decided_at: string | null
  created_at: string
}

export interface FeedbackRow {
  id: string
  user_id: string
  scope: "tool_run" | "agent_run"
  conversation_id: string | null
  run_id: string | null
  tool_execution_id: string | null
  tool_id: string | null
  agent_id: string | null
  rating: -1 | 1
  comment: string
  status: "open" | "addressed" | "dismissed"
  addressed_by_proposal_id: string | null
  subject_key: string
  created_at: string
  updated_at: string
}

export interface ChangeProposal {
  id: string
  resource: "tool" | "agent"
  action: "create" | "update"
  target_id: string | null
  status: "pending" | "applied" | "rejected"
  title: string
  rationale: string
  payload: Record<string, unknown>
  before: Record<string, unknown> | null
  feedback_ids: string[]
  created_by_user_id: string | null
  created_by_agent_id: string | null
  conversation_id: string | null
  reviewed_by: string | null
  reviewed_at: string | null
  applied_at: string | null
  apply_error: string | null
  created_at: string
  updated_at: string
}

export interface ToolBuild {
  id: string
  tool_id: string
  revision: number
  language: string
  source: string
  runtime: "wasm" | "native"
  status: "pending" | "building" | "built" | "failed"
  storage_key: string | null
  sha256: string | null
  size_bytes: number | null
  build_log: string | null
  created_by: string | null
  created_at: string
  finished_at: string | null
}

/** Persisted `data-run` part. */
export interface RunMeta {
  runId: string
  agentId: string
  agentSlug: string
  modelId: string
}

export interface ToolExecution {
  id: string
  run_id: string | null
  conversation_id: string
  tool_call_id: string
  tool_id: string | null
  tool_name: string
  input: unknown
  output: unknown
  status: "ok" | "error" | "denied" | "timeout"
  error_text: string | null
  duration_ms: number | null
  resources_used: unknown
  created_at: string
}

// ---------- AI message helpers ----------

export type ToolUIPartState =
  | "input-streaming"
  | "input-available"
  | "approval-requested"
  | "approval-responded"
  | "output-available"
  | "output-error"
  | "output-denied"

export function messageText(message: UIMessage): string {
  return message.parts
    .filter((p): p is Extract<UIMessage["parts"][number], { type: "text" }> => p.type === "text")
    .map((p) => p.text)
    .join("\n")
}

/** Read the persisted run id from a `data-run` part. */
export function messageRunId(message: UIMessage): string | null {
  for (const part of message.parts) {
    if (part.type === "data-run") {
      const data = (part as { data?: RunMeta }).data
      if (data?.runId) return data.runId
    }
  }
  return null
}

/** Read the token-usage snapshot from a `data-usage` part, if present. */
export function messageUsage(message: UIMessage): UsageSnapshot | null {
  for (const part of message.parts) {
    if (part.type === "data-usage") {
      const data = (part as { data?: UsageSnapshot }).data
      if (data) return data
    }
  }
  return null
}
