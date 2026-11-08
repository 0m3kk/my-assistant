import type { ReactNode } from "react"

import { ArtifactCard, type ArtifactData } from "./FileViews"
import { SwitchBanner, type SwitchData } from "./SwitchBanner"
import { ProposalCard } from "./ProposalCard"
import { SearchSources } from "./SearchSources"

/**
 * Part renderer registry — the "custom UI" extension point.
 *
 * A data part streamed as `data-<kind>` renders through the component
 * registered here. Adding a new custom chat UI = one component + one entry.
 */
const dataPartRenderers = new Map<string, (data: unknown) => ReactNode>()

export function registerDataPart(kind: string, renderer: (data: unknown) => ReactNode) {
  dataPartRenderers.set(kind, renderer)
}

export function dataPartRenderer(kind: string): ((data: unknown) => ReactNode) | undefined {
  return dataPartRenderers.get(kind)
}

// ---------- Tool-output renderers (registry keyed by tool name) ----------

const toolOutputRenderers = new Map<string, (output: unknown, input: unknown) => ReactNode>()

export function registerToolOutput(toolName: string, renderer: (output: unknown, input: unknown) => ReactNode) {
  toolOutputRenderers.set(toolName, renderer)
}

export function toolOutputRenderer(toolName: string): ((output: unknown, input: unknown) => ReactNode) | undefined {
  return toolOutputRenderers.get(toolName)
}

// Rendered inside the tool trace's collapsible body (keeps the trace line +
// feedback controls), instead of the raw JSON fallback.
const toolDetailRenderers = new Map<string, (output: unknown, input: unknown) => ReactNode>()

export function registerToolDetail(toolName: string, renderer: (output: unknown, input: unknown) => ReactNode) {
  toolDetailRenderers.set(toolName, renderer)
}

export function toolDetailRenderer(toolName: string): ((output: unknown, input: unknown) => ReactNode) | undefined {
  return toolDetailRenderers.get(toolName)
}

// ---------- Built-in demo: data-weather ----------

interface WeatherData {
  city?: string
  temperature_c?: number
  condition?: string
  note?: string
}

function WeatherCard({ data }: { data: WeatherData }) {
  return (
    <div className="my-1 w-fit max-w-sm rounded-xl border bg-gradient-to-br from-sky-100 to-blue-200 p-4 text-sky-950 dark:from-sky-950 dark:to-blue-900 dark:text-sky-100">
      <div className="flex items-center justify-between gap-6">
        <div>
          <p className="text-sm opacity-80">{data.city ?? "Weather"}</p>
          <p className="text-3xl font-semibold">{data.temperature_c != null ? `${data.temperature_c}°C` : "—"}</p>
          <p className="text-sm">{data.condition ?? ""}</p>
        </div>
        <span className="text-4xl" aria-hidden>
          {data.condition?.toLowerCase().includes("rain") ? "🌧️" : "⛅"}
        </span>
      </div>
      {data.note && <p className="mt-2 text-xs opacity-70">{data.note}</p>}
    </div>
  )
}

registerDataPart("weather", (data) => <WeatherCard data={(data ?? {}) as WeatherData} />)

// ---------- agent switch banner ----------

registerDataPart("switch", (data) => <SwitchBanner data={(data ?? {}) as SwitchData} />)

// ---------- save_artifact tool output → artifact download card ----------

registerToolOutput("save_artifact", (output) => {
  const artifact = (output as { artifact?: ArtifactData } | null)?.artifact
  return artifact ? <ArtifactCard artifact={artifact} /> : null
})

// ---------- meta proposals → review card ----------

registerToolOutput("meta.propose_tool", (output) => <ProposalCard data={output} />)
registerToolOutput("meta.propose_agent", (output) => <ProposalCard data={output} />)

// ---------- web_search → sources list inside the tool trace ----------

registerToolDetail("web_search", (output) => <SearchSources data={output} />)
