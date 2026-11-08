import i18n from "i18next"
import { initReactI18next } from "react-i18next"

import en from "../../locales/en.json"

const LOCALE_KEY = "assistant_locale"

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en } },
  lng: localStorage.getItem(LOCALE_KEY) ?? "en",
  fallbackLng: "en",
  interpolation: { escapeValue: false },
})

export function setLocale(locale: string) {
  localStorage.setItem(LOCALE_KEY, locale)
  void i18n.changeLanguage(locale)
}

export function currentLocale(): string {
  return i18n.language
}
