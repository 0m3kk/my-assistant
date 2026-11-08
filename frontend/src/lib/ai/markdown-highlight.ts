/**
 * Shiki highlighter singleton for chat markdown.
 *
 * A single highlighter is created lazily on first code block render with a
 * fixed set of common languages; unknown fence languages fall back to plain.
 * Dual themes (light + dark) are rendered together as CSS variables and
 * switched by the app's `.dark` class (see index.css) — no re-highlight on
 * theme change. Results are memoized per (lang, code) so stable blocks are
 * not re-highlighted while the rest of a message streams.
 */
import { createHighlighter, type Highlighter } from "shiki"

export const SHIKI_THEMES = {
  light: "github-light-default",
  dark: "github-dark-default",
} as const

const PRELOADED_LANGS = [
  "typescript",
  "javascript",
  "tsx",
  "jsx",
  "json",
  "bash",
  "shellscript",
  "rust",
  "python",
  "sql",
  "css",
  "html",
  "markdown",
  "yaml",
  "toml",
  "go",
  "java",
  "diff",
]

let highlighterPromise: Promise<Highlighter> | null = null

export function getHighlighter(): Promise<Highlighter> {
  highlighterPromise ??= createHighlighter({
    themes: [SHIKI_THEMES.light, SHIKI_THEMES.dark],
    langs: PRELOADED_LANGS,
  })
  return highlighterPromise
}

const CACHE_LIMIT = 200
const cache = new Map<string, string>()

/** Highlight fenced code to dual-theme HTML (`<pre class="shiki">…`). */
export async function highlightToHtml(code: string, lang: string): Promise<string> {
  const key = `${lang}\u0000${code}`
  const cached = cache.get(key)
  if (cached !== undefined) return cached
  const highlighter = await getHighlighter()
  const trimmed = code.replace(/\n$/, "")
  // Unknown language or alias → fall back to plain, so a bad fence never
  // breaks the chat.
  let html: string
  try {
    html = highlighter.codeToHtml(trimmed, {
      lang,
      themes: SHIKI_THEMES,
      defaultColor: "light",
    })
  } catch {
    html = highlighter.codeToHtml(trimmed, {
      lang: "text",
      themes: SHIKI_THEMES,
      defaultColor: "light",
    })
  }
  if (cache.size >= CACHE_LIMIT) cache.clear()
  cache.set(key, html)
  return html
}
