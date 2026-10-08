import type { ChartMode } from '@space-lens/web-ui'

export const CHART_MODE_KEY = 'spacelens.chartMode'

export function parseChartMode(stored: string | null): ChartMode {
  return stored === 'flat' || stored === 'nested' || stored === 'sunburst' ? stored : 'sunburst'
}
