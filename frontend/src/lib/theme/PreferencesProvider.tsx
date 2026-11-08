import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react"
import type { ReactNode } from "react"

import { useAuth } from "../auth/AuthContext"
import { preferencesApi } from "../api/endpoints"
import { currentLocale, setLocale } from "../i18n"

export type Theme = "system" | "light" | "dark"
export type FontScale = "sm" | "md" | "lg" | "xl"

const THEME_KEY = "assistant_theme"
const FONT_KEY = "assistant_font_scale"

const FONT_PX: Record<FontScale, string> = { sm: "14px", md: "16px", lg: "18px", xl: "20px" }

interface PreferencesContextValue {
  theme: Theme
  fontScale: FontScale
  setTheme: (t: Theme) => void
  setFontScale: (f: FontScale) => void
}

const PreferencesContext = createContext<PreferencesContextValue | null>(null)

function applyTheme(theme: Theme) {
  const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches)
  document.documentElement.classList.toggle("dark", dark)
}

function applyFont(scale: FontScale) {
  document.documentElement.style.setProperty("--font-size-base", FONT_PX[scale])
}

export function PreferencesProvider({ children }: { children: ReactNode }) {
  const { user } = useAuth()
  const [theme, setThemeState] = useState<Theme>(() => (localStorage.getItem(THEME_KEY) as Theme | null) ?? "system")
  const [fontScale, setFontScaleState] = useState<FontScale>(
    () => (localStorage.getItem(FONT_KEY) as FontScale | null) ?? "md",
  )

  // Apply on mount + follow OS changes when in system mode.
  useEffect(() => {
    applyTheme(theme)
    applyFont(fontScale)
    const mq = window.matchMedia("(prefers-color-scheme: dark)")
    const onChange = () => applyTheme((localStorage.getItem(THEME_KEY) as Theme | null) ?? "system")
    mq.addEventListener("change", onChange)
    return () => mq.removeEventListener("change", onChange)
  }, [theme, fontScale])

  // Sync per-account preferences when signed in.
  useEffect(() => {
    if (!user) return
    preferencesApi
      .get()
      .then((prefs) => {
        localStorage.setItem(THEME_KEY, prefs.theme)
        localStorage.setItem(FONT_KEY, prefs.font_scale)
        setThemeState(prefs.theme)
        setFontScaleState(prefs.font_scale)
        if (prefs.locale !== currentLocale()) setLocale(prefs.locale)
      })
      .catch(() => undefined)
  }, [user])

  const setTheme = useCallback(
    (t: Theme) => {
      setThemeState(t)
      localStorage.setItem(THEME_KEY, t)
      if (user) void preferencesApi.update({ theme: t }).catch(() => undefined)
    },
    [user],
  )

  const setFontScale = useCallback(
    (f: FontScale) => {
      setFontScaleState(f)
      localStorage.setItem(FONT_KEY, f)
      if (user) void preferencesApi.update({ font_scale: f }).catch(() => undefined)
    },
    [user],
  )

  const value = useMemo<PreferencesContextValue>(
    () => ({ theme, fontScale, setTheme, setFontScale }),
    [theme, fontScale, setTheme, setFontScale],
  )

  return <PreferencesContext.Provider value={value}>{children}</PreferencesContext.Provider>
}

export function usePreferences(): PreferencesContextValue {
  const ctx = useContext(PreferencesContext)
  if (!ctx) throw new Error("usePreferences must be used within PreferencesProvider")
  return ctx
}
