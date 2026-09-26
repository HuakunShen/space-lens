import type en from './locales/en.js'
export type LensLocale = 'en' | 'zh-Hans' | 'ja' | 'es' | 'fr'
export type LensKey = keyof typeof en
export type LensCatalog = Readonly<Record<LensKey, string>>
export type LensTranslator = (key: LensKey, values?: Readonly<Record<string, string | number>>) => string
