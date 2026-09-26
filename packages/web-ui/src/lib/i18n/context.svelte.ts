import { getContext, setContext } from 'svelte'
import { formatBytes, formatCount, formatExactTime, formatRelativeTime } from '../format.js'
import { selectLocale, translate } from './catalog.js'
import type { LensLocale, LensTranslator } from './types.js'

const KEY = Symbol.for('space-lens.view-i18n')

export function createLensI18n(input?: string) {
  let locale = $state<LensLocale>(selectLocale(input))
  const t: LensTranslator = (key, values) => translate(locale, key, values)
  return {
    get locale() { return locale },
    setLocale: (input: string) => { locale = selectLocale(input) },
    t,
    bytes: (value: number | string) => formatBytes(value, locale),
    count: (value: number | bigint) => formatCount(value, locale),
    absoluteTime: (value: string) => formatExactTime(value, locale),
    relativeTime: (value: string, now?: number) => formatRelativeTime(value, locale, now),
  }
}

export function provideLensI18n(input?: string) {
  const value = createLensI18n(input)
  setContext(KEY, value)
  return value
}

export function useLensI18n() {
  return getContext<ReturnType<typeof createLensI18n>>(KEY) ?? createLensI18n()
}
