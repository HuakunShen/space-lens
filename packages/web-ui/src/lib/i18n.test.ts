import { describe, expect, it } from 'vitest'
import { catalogs, selectLocale, translate } from './i18n/catalog.js'
import { formatBytes, formatCount, formatExactTime, formatRelativeTime } from './format.js'

describe('Space Lens view catalogs', () => {
  it('has exact non-empty key parity in all five locales', () => {
    const keys = Object.keys(catalogs.en).sort()
    for (const catalog of Object.values(catalogs)) {
      expect(Object.keys(catalog).sort()).toEqual(keys)
      expect(Object.values(catalog).every((value) => value.trim().length > 0)).toBe(true)
      for (const key of keys) {
        const placeholders = (value: string) => [...value.matchAll(/\{([a-zA-Z]+)\}/g)].map((match) => match[1]).sort()
        expect(placeholders(catalog[key as keyof typeof catalog])).toEqual(placeholders(catalogs.en[key as keyof typeof catalogs.en]))
      }
    }
  })

  it('selects the host locale and falls back to English for standalone views', () => {
    expect(selectLocale('zh-Hans')).toBe('zh-Hans')
    expect(selectLocale(undefined)).toBe('en')
    expect(translate('zh-Hans', 'lens.chart.title')).not.toBe(translate('en', 'lens.chart.title'))
  })

  it('formats counts, bytes, exact timestamps, and relative time with Intl locale rules', () => {
    expect(formatCount(12345, 'fr')).not.toBe(formatCount(12345, 'en'))
    expect(formatBytes(1536, 'fr')).toContain('KB')
    expect(formatExactTime('1735689600000', 'ja')).not.toBe('—')
    expect(formatRelativeTime('1735689600000', 'en', 1735689600000)).toBe('now')
    expect(formatExactTime('not-a-timestamp', 'en')).toBe('—')
  })
})
