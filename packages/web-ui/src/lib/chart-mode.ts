/**
 * The chart families the lens can draw, and how a stored preference maps
 * back onto them. Kept apart from any one chart module so the toggle, the
 * app shells, and every geometry builder agree on one vocabulary.
 */

/** The five families. Two predate the 2026-10 chart explorations. */
export type ChartMode = 'sunburst' | 'treemap' | 'icicle' | 'bubbles' | 'strips'

/**
 * The treemap's second axis: one layer of tiles, or tiles nested inside
 * their parent. Stored separately from the mode so switching family and
 * back keeps the choice.
 */
export type TreemapDensity = 'flat' | 'nested'

export const CHART_MODES: readonly ChartMode[] = ['sunburst', 'treemap', 'icicle', 'bubbles', 'strips']

export const DEFAULT_CHART_MODE: ChartMode = 'sunburst'
export const DEFAULT_TREEMAP_DENSITY: TreemapDensity = 'nested'

/**
 * Mode ids that used to be modes in their own right, before the treemap
 * gained a density axis: `flat` and `nested` were siblings of `sunburst`,
 * and `tree` was an early spelling of the nested view.
 */
const LEGACY_TREEMAP_IDS: Record<string, TreemapDensity> = {
  flat: 'flat',
  nested: 'nested',
  tree: 'nested',
}

/** Narrows a stored string to a mode id the charts actually implement. */
function isChartMode(value: string): value is ChartMode {
  return (CHART_MODES as readonly string[]).includes(value)
}

/**
 * Reads the persisted mode. Legacy `flat`/`nested`/`tree` values upscale to
 * the treemap family — call [`parseTreemapDensity`] with the same raw value
 * to recover which density they meant. Anything unrecognised falls back to
 * the sunburst rather than throwing the user into an empty chart.
 */
export function parseChartMode(stored: string | null): ChartMode {
  if (stored === null) return DEFAULT_CHART_MODE
  if (isChartMode(stored)) return stored
  return stored in LEGACY_TREEMAP_IDS ? 'treemap' : DEFAULT_CHART_MODE
}

/**
 * Reads the persisted treemap density. `legacyMode` is the raw mode key, so
 * an install that only ever stored `spacelens.chartMode=nested` keeps the
 * nested view instead of silently flattening to the default.
 */
export function parseTreemapDensity(stored: string | null, legacyMode: string | null = null): TreemapDensity {
  if (stored === 'flat' || stored === 'nested') return stored
  if (legacyMode !== null && legacyMode in LEGACY_TREEMAP_IDS) {
    return LEGACY_TREEMAP_IDS[legacyMode] ?? DEFAULT_TREEMAP_DENSITY
  }
  return DEFAULT_TREEMAP_DENSITY
}
