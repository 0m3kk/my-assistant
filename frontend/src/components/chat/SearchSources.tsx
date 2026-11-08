import { useTranslation } from "react-i18next"
import { ExternalLinkIcon } from "lucide-react"

interface SearchResult {
  title?: string
  url?: string
  snippet?: string
  published_at?: string
}

export interface SearchOutput {
  provider?: string
  query?: string
  results?: SearchResult[]
  answers?: string[]
  suggestions?: string[]
}

function domainOf(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "")
  } catch {
    return ""
  }
}

function shortDate(value: string): string {
  return value.length >= 10 ? value.slice(0, 10) : value
}

/** `web_search` output: answers (if any) + ranked sources with snippets. */
export function SearchSources({ data }: { data: unknown }) {
  const { t } = useTranslation()
  const output = (data ?? {}) as SearchOutput
  const results = Array.isArray(output.results) ? output.results : []
  const answers = Array.isArray(output.answers) ? output.answers : []
  const suggestions = Array.isArray(output.suggestions) ? output.suggestions : []

  if (results.length === 0 && answers.length === 0) {
    return <p className="text-xs text-muted-foreground">{t("tool.noResults")}</p>
  }

  return (
    <div className="grid gap-2">
      {answers.map((answer, i) => (
        <p key={i} className="rounded-md border bg-muted/30 px-3 py-2 text-xs leading-relaxed">
          {answer}
        </p>
      ))}

      {results.length > 0 && (
        <div className="grid gap-1.5">
          <p className="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
            {t("tool.sources")}
          </p>
          {results.map((result, i) => (
            <div key={`${result.url ?? i}`} className="rounded-md border px-3 py-2">
              <a
                href={result.url}
                target="_blank"
                rel="noreferrer"
                className="flex items-center gap-1 text-xs font-medium underline underline-offset-4"
              >
                <span className="truncate">{result.title || result.url}</span>
                <ExternalLinkIcon className="size-3 shrink-0 opacity-60" />
              </a>
              <div className="mt-0.5 flex items-center gap-2 text-[11px] text-muted-foreground">
                {result.url && <span className="truncate">{domainOf(result.url)}</span>}
                {result.published_at && (
                  <span className="shrink-0">{shortDate(result.published_at)}</span>
                )}
              </div>
              {result.snippet && (
                <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">{result.snippet}</p>
              )}
            </div>
          ))}
        </div>
      )}

      {suggestions.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {suggestions.map((suggestion, i) => (
            <span
              key={i}
              className="rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground"
            >
              {suggestion}
            </span>
          ))}
        </div>
      )}
    </div>
  )
}
