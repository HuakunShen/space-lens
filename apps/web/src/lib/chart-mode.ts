import {
  DEFAULT_CHART_MODE,
  DEFAULT_TREEMAP_DENSITY,
  parseChartMode as parseStoredMode,
  parseTreemapDensity as parseStoredDensity,
  type ChartMode,
  type TreemapDensity,
} from '@space-lens/web-ui'

export const CHART_MODE_KEY = 'spacelens.chartMode'
export const TREEMAP_DENSITY_KEY = 'spacelens.treemapDensity'

/**
 * The mode and the treemap density are separate keys so switching family and
 * back keeps the density. Both readers live in the component library, where
 * the upgrade rules for pre-treemap installs are tested; this module only
 * owns the key names the app persists under.
 */
export function parseChartMode(stored: string | null): ChartMode {
  return parseStoredMode(stored)
}

export function parseTreemapDensity(stored: string | null, legacyMode: string | null = null): TreemapDensity {
  return parseStoredDensity(stored, legacyMode)
}

export { DEFAULT_CHART_MODE, DEFAULT_TREEMAP_DENSITY }
export type { ChartMode, TreemapDensity }
