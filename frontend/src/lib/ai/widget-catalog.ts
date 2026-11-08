import { defineCatalog } from "@json-render/core"
import { schema } from "@json-render/react"
import { z } from "zod"

/**
 * Widget catalog — the set of components the model may build UIs from.
 *
 * The model learns about these through the json-render section of the system
 * prompt, generated Rust-side in `backend/src/agent/jsonrender.rs`
 * (INLINE_SYSTEM_PROMPT) — keep the two in sync (names, props, semantics).
 */
export const widgetCatalog = defineCatalog(schema, {
  components: {
    Card: {
      props: z.object({
        title: z.string(),
        description: z.string().nullish(),
      }),
      slots: ["default"],
      description: "Titled container that groups related widgets.",
    },
    Stack: {
      props: z.object({
        direction: z.enum(["vertical", "horizontal"]).nullish(),
        gap: z.enum(["sm", "md", "lg"]).nullish(),
      }),
      slots: ["default"],
      description: "Layout container stacking its children.",
    },
    Text: {
      props: z.object({
        content: z.string(),
      }),
      description: "Markdown text block.",
    },
    Metric: {
      props: z.object({
        label: z.string(),
        value: z.string(),
        delta: z.string().nullish(),
      }),
      description: "Single KPI: label, big value, optional delta like +12.4%.",
    },
    Table: {
      props: z.object({
        headers: z.array(z.string()),
        rows: z.array(z.array(z.union([z.string(), z.number()]))),
        caption: z.string().nullish(),
      }),
      description: "Data table with headers and string/number rows.",
    },
    Chart: {
      props: z.object({
        type: z.enum(["bar", "line", "area"]),
        title: z.string().nullish(),
        categories: z.array(z.string()),
        series: z.array(z.object({ name: z.string(), values: z.array(z.number()) })),
      }),
      description:
        "Bar/line/area chart; every series has exactly one value per category, in category order.",
    },
  },
  // The React schema types actions as a required catalog key; the v1 catalog
  // is display-only, so it declares none.
  actions: {},
})
