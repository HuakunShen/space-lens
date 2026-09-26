import en from './locales/en.js'
import zhHans from './locales/zh-Hans.js'
import ja from './locales/ja.js'
import es from './locales/es.js'
import fr from './locales/fr.js'
import type { LensCatalog, LensKey, LensLocale } from './types.js'

export const catalogs: Readonly<Record<LensLocale, LensCatalog>> = {
  en, 'zh-Hans': zhHans, ja, es, fr,
}

export function selectLocale(value: string | undefined): LensLocale {
  return value === 'zh-Hans' || value === 'ja' || value === 'es' || value === 'fr' ? value : 'en'
}

export function translate(locale: LensLocale, key: LensKey, values: Readonly<Record<string, string | number>> = {}): string {
  return catalogs[locale][key].replace(/\{([a-zA-Z]+)\}/g, (placeholder, name: string) =>
    Object.hasOwn(values, name) ? String(values[name]) : placeholder)
}
