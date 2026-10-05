export type InterfaceStyle = 'auto' | 'web' | 'macos' | 'windows' | 'linux'
export type Density = 'compact' | 'comfortable'
export interface Appearance {
  style: InterfaceStyle
  density: Density
}
export const APPEARANCE_KEY = 'spacelens.appearance'
export function parseAppearance(raw: string | null): Appearance {
  try {
    const value: unknown = JSON.parse(raw ?? '{}')
    if (typeof value !== 'object' || value === null) return { style: 'auto', density: 'compact' }
    const style = 'style' in value ? value.style : null
    const density = 'density' in value ? value.density : null
    return {
      style: style === 'web' || style === 'macos' || style === 'windows' || style === 'linux' ? style : 'auto',
      density: density === 'comfortable' ? density : 'compact',
    }
  } catch {
    return { style: 'auto', density: 'compact' }
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
