import { api } from "./client"
import type {
  Agent,
  AgentGroup,
  AgentOption,
  AgentToolAssignment,
  Approval,
  AuthResponse,
  ChangeProposal,
  ConversationGroup,
  ConversationSummary,
  DiscoveredModel,
  FeedbackRow,
  FileDto,
  Meta,
  Model,
  ModelOption,
  ReasoningEffort,
  Provider,
  ProviderPreset,
  PublicUser,
  SecretPublic,
  StoredMessage,
  ToolBuild,
  ToolExecution,
  ToolGroup,
  ToolRow,
  UserPreferences,
  WorkspaceEntry,
} from "../types"

let metaPromise: Promise<Meta> | null = null

export const metaApi = {
  /** Public pre-login metadata; fetched once per page load. */
  get: () => (metaPromise ??= api.get<Meta>("/meta")),
}

export const authApi = {
  register: (body: { email: string; password: string; name: string }) =>
    api.post<AuthResponse>("/auth/register", body),
  login: (body: { email: string; password: string }) => api.post<AuthResponse>("/auth/login", body),
  me: () => api.get<PublicUser>("/auth/me"),
  updateProfile: (body: { name?: string; avatar_url?: string | null }) =>
    api.put<PublicUser>("/auth/me", body),
}

export const preferencesApi = {
  get: () => api.get<UserPreferences>("/me/preferences"),
  update: (
    body: Partial<Pick<UserPreferences, "theme" | "font_scale" | "locale" | "title_model_id">>,
  ) => api.put<UserPreferences>("/me/preferences", body),
}

export const conversationsApi = {
  list: () => api.get<ConversationSummary[]>("/conversations"),
  messages: (id: string) => api.get<StoredMessage[]>(`/conversations/${id}/messages`),
  rename: (id: string, title: string) => api.put<ConversationSummary>(`/conversations/${id}`, { title }),
  setGroup: (id: string, groupId: string | null) =>
    api.put<ConversationSummary>(`/conversations/${id}/group`, { group_id: groupId }),
  generateTitle: (id: string) => api.post<ConversationSummary>(`/conversations/${id}/title`, {}),
  cancel: (id: string) => api.post<{ cancelled: boolean }>("/chat/cancel", { conversation_id: id }),
  remove: (id: string) => api.delete<void>(`/conversations/${id}`),
  approvals: (id: string) => api.get<Approval[]>(`/conversations/${id}/approvals`),
}

export const conversationGroupsApi = {
  list: () => api.get<ConversationGroup[]>("/conversation-groups"),
  create: (name: string) => api.post<ConversationGroup>("/conversation-groups", { name }),
  rename: (id: string, name: string) =>
    api.put<ConversationGroup>(`/conversation-groups/${id}`, { name }),
  remove: (id: string) => api.delete<void>(`/conversation-groups/${id}`),
}

export const filesApi = {
  /** Upload one attachment; the returned row is linked when the message is sent. */
  upload: (file: File) => {
    const form = new FormData()
    form.append("file", file)
    return api.upload<FileDto>("/files", form)
  },
  remove: (id: string) => api.delete<void>(`/files/${id}`),
}

export const workspaceApi = {
  /** Recursive file tree of the conversation sandbox. */
  tree: (conversationId: string) =>
    api.get<WorkspaceEntry[]>(`/conversations/${conversationId}/workspace`),
}

export const modelsApi = {
  list: () => api.get<ModelOption[]>("/models"),
}

export const agentsApi = {
  /** Chat agents the signed-in user may start a conversation with. */
  list: () => api.get<AgentOption[]>("/agents"),
}

export const approvalsApi = {
  decide: (
    id: string,
    decision: "approved" | "denied",
    decisionMode?: "once" | "propose_update",
  ) => api.post<Approval>(`/approvals/${id}`, { decision, decision_mode: decisionMode }),
}

export const feedbackApi = {
  submit: (body: {
    scope: "tool_run" | "agent_run"
    conversation_id: string
    run_id?: string
    tool_call_id?: string
    rating: -1 | 1
    comment?: string
  }) => api.post<FeedbackRow>("/feedback", body),
  list: (conversationId: string) =>
    api.get<{ feedback: FeedbackRow[] }>(`/feedback?conversation_id=${conversationId}`),
  remove: (id: string) => api.delete<void>(`/feedback/${id}`),
}

export const adminApi = {
  providerPresets: () => api.get<ProviderPreset[]>("/admin/provider-presets"),
  providers: {
    list: () => api.get<{ providers: Provider[] }>("/admin/providers"),
    create: (body: { name: string; preset: string; base_url: string; api_key?: string; secret_name?: string }) =>
      api.post<Provider>("/admin/providers", body),
    update: (
      id: string,
      body: { name?: string; base_url?: string; status?: string; api_key?: string; secret_name?: string },
    ) => api.put<Provider>(`/admin/providers/${id}`, body),
    remove: (id: string) => api.delete<void>(`/admin/providers/${id}`),
  },
  models: {
    list: (providerId: string) => api.get<{ models: Model[] }>(`/admin/providers/${providerId}/models`),
    /** Query the provider's `/models` endpoint (live network call upstream). */
    discover: (providerId: string) =>
      api.get<{ models: DiscoveredModel[] }>(`/admin/providers/${providerId}/models/discover`),
    /** Idempotently add discovered models; already-configured names are skipped. */
    importDiscovered: (providerId: string, apiNames: string[]) =>
      api.post<{ created: Model[]; created_count: number; skipped_count: number }>(
        `/admin/providers/${providerId}/models/import`,
        { api_names: apiNames },
      ),
    create: (body: {
      provider_id: string
      api_name: string
      display_name?: string
      context_window?: number | null
      capabilities?: Record<string, boolean>
      reasoning_effort?: ReasoningEffort | null
      is_default?: boolean
    }) => api.post<Model>(`/admin/providers/${body.provider_id}/models`, body),
    update: (
      id: string,
      body: {
        api_name?: string
        display_name?: string
        capabilities?: Record<string, boolean>
        reasoning_effort?: ReasoningEffort | null
        is_default?: boolean
        status?: string
        context_window?: number | null
      },
    ) => api.put<Model>(`/admin/models/${id}`, body),
    remove: (id: string) => api.delete<void>(`/admin/models/${id}`),
  },
  agents: {
    list: () => api.get<{ agents: Agent[] }>("/admin/agents"),
    create: (body: Partial<Agent> & { name: string; slug: string }) => api.post<Agent>("/admin/agents", body),
    update: (id: string, body: Partial<Agent>) => api.put<Agent>(`/admin/agents/${id}`, body),
    remove: (id: string) => api.delete<void>(`/admin/agents/${id}`),
    tools: (id: string) => api.get<{ tools: AgentToolAssignment[] }>(`/admin/agents/${id}/tools`),
    assignTool: (
      id: string,
      body: { tool_id: string; requires_approval?: boolean | null; config?: Record<string, unknown> | null },
    ) => api.post<void>(`/admin/agents/${id}/tools`, body),
    unassignTool: (id: string, toolId: string) => api.delete<void>(`/admin/agents/${id}/tools/${toolId}`),
  },
  tools: {
    list: () => api.get<{ tools: ToolRow[] }>("/admin/tools"),
    create: (body: {
      name: string
      display_name?: string
      description?: string
      kind: string
      config: Record<string, unknown>
      parameters_schema: Record<string, unknown>
      requires_approval?: boolean
    }) => api.post<ToolRow>("/admin/tools", body),
    update: (id: string, body: Partial<ToolRow>) => api.put<ToolRow>(`/admin/tools/${id}`, body),
    remove: (id: string) => api.delete<void>(`/admin/tools/${id}`),
    builds: (id: string) => api.get<{ builds: ToolBuild[] }>(`/admin/tools/${id}/builds`),
    build: (id: string) => api.post<ToolBuild>(`/admin/tools/${id}/build`, {}),
  },
  toolGroups: {
    list: () => api.get<{ groups: ToolGroup[] }>("/admin/tool-groups"),
    create: (name: string) => api.post<ToolGroup>("/admin/tool-groups", { name }),
    update: (id: string, name: string) => api.put<ToolGroup>(`/admin/tool-groups/${id}`, { name }),
    remove: (id: string) => api.delete<void>(`/admin/tool-groups/${id}`),
  },
  agentGroups: {
    list: () => api.get<{ groups: AgentGroup[] }>("/admin/agent-groups"),
    create: (name: string) => api.post<AgentGroup>("/admin/agent-groups", { name }),
    update: (id: string, name: string) => api.put<AgentGroup>(`/admin/agent-groups/${id}`, { name }),
    remove: (id: string) => api.delete<void>(`/admin/agent-groups/${id}`),
  },
  proposals: {
    list: (params?: { status?: string; resource?: string }) =>
      api.get<{ proposals: ChangeProposal[] }>(
        `/admin/proposals${params ? `?${new URLSearchParams(params as Record<string, string>).toString()}` : ""}`,
      ),
    get: (id: string) => api.get<ChangeProposal>(`/admin/proposals/${id}`),
    apply: (id: string, body: { payload?: Record<string, unknown>; force?: boolean }) =>
      api.post<ChangeProposal>(`/admin/proposals/${id}/apply`, body),
    reject: (id: string) => api.post<ChangeProposal>(`/admin/proposals/${id}/reject`, {}),
  },
  feedback: {
    list: (params?: { status?: string; tool_id?: string; agent_id?: string; limit?: number }) =>
      api.get<{ feedback: FeedbackRow[] }>(
        `/admin/feedback${params ? `?${new URLSearchParams({
          ...params,
          limit: params.limit != null ? String(params.limit) : undefined,
        } as Record<string, string>).toString()}` : ""}`,
      ),
    update: (id: string, status: "open" | "addressed" | "dismissed") =>
      api.patch<FeedbackRow>(`/admin/feedback/${id}`, { status }),
  },
  secrets: {
    list: () => api.get<{ secrets: SecretPublic[] }>("/admin/secrets"),
    create: (body: { name: string; description: string; value: string }) =>
      api.post<SecretPublic>("/admin/secrets", body),
    update: (id: string, body: { description?: string; value?: string }) =>
      api.put<SecretPublic>(`/admin/secrets/${id}`, body),
    remove: (id: string) => api.delete<void>(`/admin/secrets/${id}`),
  },
  executions: {
    list: (params?: { tool_id?: string; limit?: number }) =>
      api.get<{ executions: ToolExecution[] }>(
        `/admin/executions${params ? `?${new URLSearchParams({
          ...params,
          limit: params.limit != null ? String(params.limit) : undefined,
        } as Record<string, string>).toString()}` : ""}`,
      ),
  },
}
