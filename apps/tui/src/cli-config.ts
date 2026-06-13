import type { CliOptions, Preset, SortMode } from './model.js'

export const PRESETS = ['node', 'rust', 'gitignored'] as const satisfies readonly Preset[]
export const SORT_MODES = ['size', 'path'] as const satisfies readonly SortMode[]

export interface RawCliConfig {
  readonly paths: readonly string[]
  readonly presets: readonly string[]
  readonly ignoreHidden: boolean
  readonly sort: string
}

export function normalizeCliConfig(raw: RawCliConfig): CliOptions {
  return {
    paths: raw.paths.length > 0 ? [...raw.paths] : ['.'],
    presets: parsePresetList(raw.presets),
    ignoreHidden: raw.ignoreHidden,
    sort: parseSortMode(raw.sort),
  }
}

export function parsePresetList(raw: readonly string[]): Preset[] {
  const seen = new Set<Preset>()
  const presets: Preset[] = []

  for (const value of raw) {
    for (const token of value.split(',')) {
      const preset = token.trim()
      if (preset.length === 0) {
        continue
      }
      if (!isPreset(preset)) {
        throw new Error(`Unknown preset "${preset}". Use node, rust, or gitignored.`)
      }
      if (!seen.has(preset)) {
        seen.add(preset)
        presets.push(preset)
      }
    }
  }

  return presets
}

export function parseSortMode(raw: string): SortMode {
  if (!isSortMode(raw)) {
    throw new Error(`Unknown sort mode "${raw}". Use size or path.`)
  }

  return raw
}

function isPreset(value: string): value is Preset {
  return (PRESETS as readonly string[]).includes(value)
}

function isSortMode(value: string): value is SortMode {
  return (SORT_MODES as readonly string[]).includes(value)
}
