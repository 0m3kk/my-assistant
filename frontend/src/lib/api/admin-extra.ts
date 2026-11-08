import { api } from "./client"
import type { ToolBuild } from "../types"

export interface VersionRow {
  revision: number
  snapshot: Record<string, unknown>
  note: string
  created_at: string
}

export interface TestResult {
  ok: boolean
  output?: unknown
  error?: string
}

export interface RestoreResult {
  proposal_id: string
  status: string
  title: string
}

/** Build/test/version operations shared by the tool and agent admin UIs. */
export const toolOpsApi = {
  builds: (toolId: string) => api.get<{ builds: ToolBuild[] }>(`/admin/tools/${toolId}/builds`),
  build: (toolId: string) => api.post<ToolBuild>(`/admin/tools/${toolId}/build`, {}),
  test: (toolId: string, input: unknown) =>
    api.post<TestResult>(`/admin/tools/${toolId}/test`, { input }),
  versions: (toolId: string) =>
    api.get<{ versions: VersionRow[] }>(`/admin/tools/${toolId}/versions`),
  restore: (toolId: string, revision: number) =>
    api.post<RestoreResult>(`/admin/tools/${toolId}/versions/${revision}/restore`, {}),
  agentVersions: (agentId: string) =>
    api.get<{ versions: VersionRow[] }>(`/admin/agents/${agentId}/versions`),
  restoreAgent: (agentId: string, revision: number) =>
    api.post<RestoreResult>(`/admin/agents/${agentId}/versions/${revision}/restore`, {}),
}
