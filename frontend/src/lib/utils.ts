import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/** Compact token counts for labels and meters: 8192 → "8.2K", 200000 → "200K". */
export function formatTokens(tokens: number): string {
  if (tokens < 1000) return String(tokens)
  const thousands = tokens / 1000
  const digits = thousands < 10 ? 1 : 0
  return `${thousands.toFixed(digits).replace(/\.0$/, "")}K`
}

/** "8.2K" / "200K" / "1.0M" — context windows read better in these units. */
export function formatContextWindow(tokens: number): string {
  if (tokens >= 1_000_000) {
    return `${(tokens / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`
  }
  return formatTokens(tokens)
}
