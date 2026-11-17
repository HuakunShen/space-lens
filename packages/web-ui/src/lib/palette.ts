/**
 * Categorical colours for series that need to stay apart from each other: the
 * segment bar, the preset cards, the memory chart. Rank-ordered, so the first
 * entries are the ones most likely to be on screen together.
 */
export const SERIES_COLORS = [
  '#4d9bea',
  '#e452ba',
  '#38b994',
  '#d3a052',
  '#8277f4',
  '#31bdc3',
  '#df797b',
  '#aa65ef',
  '#94b670',
  '#c8759d',
  '#6ba7b8',
  '#9b87c7',
] as const

export function seriesColor(index: number): string {
  return SERIES_COLORS[index % SERIES_COLORS.length]!
}
