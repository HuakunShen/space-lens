export type InterfaceStyle = 'auto' | 'web' | 'macos' | 'windows' | 'linux'
export type Density = 'compact' | 'comfortable'
/** How see-through the window is, 0 (solid) to 100 (most transparent). */
export type Transparency = number
export const DEFAULT_TRANSPARENCY = 33
export const MAX_TRANSPARENCY = 100
/** Settings saved before the slider stored one of three names. */
const LEGACY_TRANSPARENCY: Record<string, number> = { solid: 0, balanced: DEFAULT_TRANSPARENCY, glassy: 60 }
export function parseTransparency(value: unknown): Transparency {
  if (typeof value === 'number' && Number.isFinite(value)) {
    return Math.min(MAX_TRANSPARENCY, Math.max(0, Math.round(value)))
  }
  return typeof value === 'string' ? (LEGACY_TRANSPARENCY[value] ?? DEFAULT_TRANSPARENCY) : DEFAULT_TRANSPARENCY
}
/**
 * The multiplier on every glass surface's alpha. The default lands on 1, the
 * surfaces' own tuning; 0 clamps them all to opaque, 100 keeps about 40%.
 */
export function glassStrength(transparency: Transparency): number {
  return Number((1.3 - 0.9 * (parseTransparency(transparency) / MAX_TRANSPARENCY)).toFixed(3))
}
/** Where the accent color comes from: the OS (desktop only) or the app. */
export type AccentSource = 'system' | 'default'
export interface Appearance {
  style: InterfaceStyle
  density: Density
  transparency: Transparency
  accent: AccentSource
}
export const APPEARANCE_KEY = 'spacelens.appearance'
export function parseAppearance(raw: string | null): Appearance {
  try {
    const value: unknown = JSON.parse(raw ?? '{}')
    if (typeof value !== 'object' || value === null)
      return { style: 'auto', density: 'compact', transparency: DEFAULT_TRANSPARENCY, accent: 'system' }
    const style = 'style' in value ? value.style : null
    const density = 'density' in value ? value.density : null
    const transparency = 'transparency' in value ? value.transparency : null
    const accent = 'accent' in value ? value.accent : null
    return {
      style: style === 'web' || style === 'macos' || style === 'windows' || style === 'linux' ? style : 'auto',
      density: density === 'comfortable' ? density : 'compact',
      transparency: parseTransparency(transparency),
      accent: accent === 'default' ? accent : 'system',
    }
  } catch {
    return { style: 'auto', density: 'compact', transparency: DEFAULT_TRANSPARENCY, accent: 'system' }
  }
}
export function resolveStyle(
  style: InterfaceStyle,
  desktop: boolean,
  userAgent: string,
): Exclude<InterfaceStyle, 'auto'> {
  if (style !== 'auto') return style
  if (!desktop) return 'web'
  if (/Windows/i.test(userAgent)) return 'windows'
  if (/Macintosh|Mac OS X/i.test(userAgent)) return 'macos'
  if (/Linux/i.test(userAgent) && !/Android/i.test(userAgent)) return 'linux'
  return 'web'
}
