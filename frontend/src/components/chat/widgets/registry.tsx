import type { ReactNode } from "react"
import ReactMarkdown from "react-markdown"
import remarkGfm from "remark-gfm"

import { defineRegistry } from "@json-render/react"

import { widgetCatalog } from "@/lib/ai/widget-catalog"
import { cn } from "@/lib/utils"

/**
 * json-render widget implementations — render the UI specs the model streams
 * as `data-spec` parts (see `widget-catalog.ts` for the contract, and the
 * Rust-side `INLINE_SYSTEM_PROMPT` describing it to the model).
 */

function ProseContent({ content }: { content: string }) {
  return (
    <div className="prose prose-sm dark:prose-invert max-w-none [&_p]:my-1 [&_p]:leading-relaxed">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{content}</ReactMarkdown>
    </div>
  )
}

function MetricValue({ delta }: { delta?: string | null }) {
  if (!delta) return null
  const negative = delta.trimStart().startsWith("-")
  return (
    <span className={cn("text-xs font-medium", negative ? "text-red-500" : "text-emerald-500")}>
      {delta}
    </span>
  )
}

// ---------- Chart (pure SVG — no chart dependency) ----------

const SERIES_COLORS = ["#0ea5e9", "#8b5cf6", "#10b981", "#f59e0b"]

interface ChartSeries {
  name: string
  values: number[]
}

function ChartPlot({
  type,
  categories,
  series,
}: {
  type: "bar" | "line" | "area"
  categories: string[]
  series: ChartSeries[]
}) {
  const W = 340
  const H = 170
  const padL = 6
  const padR = 6
  const padT = 8
  const padB = 20
  const plotW = W - padL - padR
  const plotH = H - padT - padB

  const flat = series.flatMap((s) => s.values)
  const rawMax = flat.length > 0 ? Math.max(...flat, 0) : 1
  const max = rawMax <= 0 ? 1 : rawMax * 1.1
  const y = (v: number) => padT + plotH - (v / max) * plotH

  const nCats = Math.max(categories.length, 1)
  const step = plotW / nCats
  const gridYs = [0, 1 / 3, 2 / 3, 1].map((f) => padT + plotH * f)
  const labelEvery = Math.ceil(nCats / 8)

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="w-full" role="img">
      {gridYs.map((gy) => (
        <line
          key={gy}
          x1={padL}
          x2={W - padR}
          y1={gy}
          y2={gy}
          className="stroke-current text-muted-foreground/20"
          strokeWidth={1}
        />
      ))}

      {type === "bar" &&
        categories.map((_, i) => {
          const groupW = step
          const barW = (groupW / Math.max(series.length, 1)) * 0.72
          return series.map((s, j) => {
            const v = s.values[i] ?? 0
            const x = padL + i * groupW + j * (groupW / Math.max(series.length, 1)) + (groupW / Math.max(series.length, 1) - barW) / 2
            const top = y(v)
            return (
              <rect
                key={`${i}-${j}`}
                x={x}
                y={top}
                width={Math.max(barW, 1)}
                height={padT + plotH - top}
                rx={2}
                fill={SERIES_COLORS[j % SERIES_COLORS.length]}
              />
            )
          })
        })}

      {(type === "line" || type === "area") &&
        series.map((s, j) => {
          const points = s.values.map((v, i) => `${padL + step * (i + 0.5)},${y(v)}`)
          const color = SERIES_COLORS[j % SERIES_COLORS.length]
          return (
            <g key={j}>
              {type === "area" && (
                <polygon
                  points={`${padL + step * 0.5},${y(0)} ${points.join(" ")} ${
                    padL + step * (s.values.length - 0.5)
                  },${y(0)}`}
                  fill={color}
                  opacity={0.15}
                />
              )}
              <polyline
                points={points.join(" ")}
                fill="none"
                stroke={color}
                strokeWidth={2}
                strokeLinejoin="round"
                strokeLinecap="round"
              />
            </g>
          )
        })}

      <line
        x1={padL}
        x2={W - padR}
        y1={y(0)}
        y2={y(0)}
        className="stroke-current text-muted-foreground/40"
        strokeWidth={1}
      />
      {categories.map((c, i) =>
        i % labelEvery === 0 ? (
          <text
            key={i}
            x={type === "bar" ? padL + i * step + step / 2 : padL + step * (i + 0.5)}
            y={H - 6}
            textAnchor="middle"
            className="fill-current text-[8px] text-muted-foreground"
          >
            {c.length > 9 ? `${c.slice(0, 8)}…` : c}
          </text>
        ) : null,
      )}
    </svg>
  )
}

function ChartLegend({ series }: { series: ChartSeries[] }) {
  if (series.length < 2) return null
  return (
    <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1">
      {series.map((s, j) => (
        <span key={s.name} className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <span
            className="inline-block size-2 rounded-full"
            style={{ backgroundColor: SERIES_COLORS[j % SERIES_COLORS.length] }}
          />
          {s.name}
        </span>
      ))}
    </div>
  )
}

// ---------- Registry ----------

export const { registry: widgetRegistry } = defineRegistry(widgetCatalog, {
  components: {
    Card: ({ props, children }) => (
      <div className="my-1 w-fit max-w-full min-w-64 rounded-xl border bg-card p-4">
        <div className="mb-3">
          <p className="text-sm font-medium">{props.title}</p>
          {props.description && (
            <p className="mt-0.5 text-xs text-muted-foreground">{props.description}</p>
          )}
        </div>
        <div className="flex flex-col gap-3">{children}</div>
      </div>
    ),

    Stack: ({ props, children }) => (
      <div
        className={cn(
          "flex",
          props.direction === "horizontal" ? "flex-row items-center" : "flex-col",
          props.gap === "sm" && "gap-1.5",
          (!props.gap || props.gap === "md") && "gap-3",
          props.gap === "lg" && "gap-6",
        )}
      >
        {children}
      </div>
    ),

    Text: ({ props }) => <ProseContent content={props.content} />,

    Metric: ({ props }) => (
      <div className="rounded-lg border bg-muted/40 px-3 py-2.5">
        <p className="text-xs text-muted-foreground">{props.label}</p>
        <div className="mt-1 flex items-baseline gap-2">
          <span className="text-2xl font-semibold tabular-nums">{props.value}</span>
          <MetricValue delta={props.delta} />
        </div>
      </div>
    ),

    Table: ({ props }) => (
      <figure className="my-1 max-w-full overflow-x-auto">
        <table className="w-full border-collapse text-sm">
          <thead>
            <tr className="border-b">
              {props.headers.map((h) => (
                <th key={h} className="px-3 py-1.5 text-left text-xs font-medium text-muted-foreground">
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {props.rows.map((row, i) => (
              <tr key={i} className="border-b last:border-0">
                {row.map((cell, j) => (
                  <td
                    key={j}
                    className={cn(
                      "px-3 py-1.5 align-top tabular-nums",
                      j === 0 ? "font-medium" : "text-muted-foreground",
                      typeof cell === "number" && "text-right",
                    )}
                  >
                    {typeof cell === "number" ? cell.toLocaleString() : cell}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
        {props.caption && (
          <figcaption className="mt-1.5 text-xs text-muted-foreground">{props.caption}</figcaption>
        )}
      </figure>
    ),

    Chart: ({ props }) => (
      <div className="my-1 w-full max-w-md rounded-xl border bg-card p-3">
        {props.title && <p className="mb-1 px-1 text-sm font-medium">{props.title}</p>}
        <ChartPlot type={props.type} categories={props.categories} series={props.series} />
        <ChartLegend series={props.series} />
      </div>
    ),
  },
})

/** Fallback for spec elements whose type is not in the registry. */
export function WidgetFallback({ children }: { children?: ReactNode }): ReactNode {
  return (
    <div className="my-1 rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
      {children ?? "Unsupported widget"}
    </div>
  )
}
