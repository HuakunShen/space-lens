import type { TreeNodeSummary } from '../types'

/**
 * What a chart says when it has nothing to paint. Shared by all five chart
 * families so an unscanned location, an empty folder, and a fresh app never
 * describe themselves differently depending on which chart is selected.
 */
export function chartEmptyMessage(focusNode: TreeNodeSummary | null): string {
  if (focusNode?.scanState === 'skipped') return 'This location was not scanned'
  return focusNode ? 'No child items to display' : 'Choose a folder to explore'
}
