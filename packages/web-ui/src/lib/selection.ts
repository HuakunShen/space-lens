import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'
import { canCollectNode, isVirtualAppPath } from './app-scan.ts'

function comparablePath(path: string): string {
  const windows = /^[a-z]:[\\/]/i.test(path) || path.startsWith('\\\\')
  const normalized = windows ? path.replaceAll('\\', '/').toLowerCase() : path
  return normalized.replace(/\/+$/, '') || '/'
}

export function isPathWithin(path: string, parent: string): boolean {
  // A10/§5: the synthetic virtual identifier is not a filesystem path — it
  // never sits within, contains, or equals a real path.
  if (isVirtualAppPath(path) || isVirtualAppPath(parent)) return false
  const child = comparablePath(path)
  const ancestor = comparablePath(parent)
  return child === ancestor || child.startsWith(ancestor === '/' ? '/' : `${ancestor}/`)
}

/**
 * A6: coverage is per-scan. Node ids are path-derived and repeat across
 * sessions, so only entries staged in `scanId` cover `path`. A null scanId
 * keeps the legacy unfiltered behavior for callers without a session.
 */
export function selectedAncestor(
  entries: readonly CollectorEntry[],
  path: string,
  scanId?: string | null,
): CollectorEntry | undefined {
  if (isVirtualAppPath(path)) return undefined
  return entries.find((entry) => (scanId == null || entry.scanId === scanId) && isPathWithin(path, entry.path))
}

export function addSelection(
  entries: readonly CollectorEntry[],
  node: TreeNodeSummary,
  scanId: string,
  addedAt: string,
  id: string,
): CollectorEntry[] {
  // A8: the single host-computed eligibility predicate decides. Virtual
  // roots, bundle leaves, accounting leaves and truncation buckets refuse
  // inside canCollectNode; partial/skipped fall back to refused for path
  // scans unless the host marks them otherwise.
  if (!canCollectNode(node)) return [...entries]
  if (selectedAncestor(entries, node.path, scanId)) return [...entries]
  return [
    ...entries.filter((entry) => entry.scanId !== scanId || !isPathWithin(entry.path, node.path)),
    { id, scanId, nodeId: node.id, path: node.path, name: node.name, size: node.size, addedAt },
  ]
}
