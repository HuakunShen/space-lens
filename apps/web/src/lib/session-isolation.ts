import type { CollectorEntry } from '@space-lens/contract'

/**
 * A6 session isolation, in a rune-free module so `node --test` can pin it.
 * The collector is scoped by scanId: switching stashes the outgoing entries
 * and restores the incoming ones; late planning responses are discarded
 * when the sequence or the active scan moved on; switching is blocked while
 * a cleanup executes (checked by the caller via `workbench.deleting`).
 */
const collectorByScan = new Map<string, CollectorEntry[]>()

export function stashCollector(scanId: string, entries: readonly CollectorEntry[]): void {
  collectorByScan.set(scanId, [...entries])
}

export function restoreCollector(scanId: string): CollectorEntry[] {
  return [...(collectorByScan.get(scanId) ?? [])]
}

export function dropCollector(scanId: string): void {
  collectorByScan.delete(scanId)
}

/** Test seam: how many collectors are currently stashed. */
export function stashedCollectorCount(): number {
  return collectorByScan.size
}

/** Test seam: resets all stashed collectors. */
export function clearStashedCollectors(): void {
  collectorByScan.clear()
}

export function shouldApplyPlan(input: {
  request: number
  sequence: number
  activeScanId: string | null
  scanId: string
}): boolean {
  return input.request === input.sequence && input.activeScanId === input.scanId
}
