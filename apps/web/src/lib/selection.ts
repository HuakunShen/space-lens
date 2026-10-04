import type { CollectorEntry, TreeNodeSummary } from '@space-lens/contract'

function comparablePath(path: string): string {
  const windows = /^[a-z]:[\\/]/i.test(path) || path.startsWith('\\\\')
  const normalized = windows ? path.replaceAll('\\', '/').toLowerCase() : path
  return normalized.replace(/\/+$/, '') || '/'
}

export function isPathWithin(path: string, parent: string): boolean {
  const child = comparablePath(path)
  const ancestor = comparablePath(parent)
  return child === ancestor || child.startsWith(ancestor === '/' ? '/' : `${ancestor}/`)
}

export function selectedAncestor(entries: readonly CollectorEntry[], path: string): CollectorEntry | undefined {
  return entries.find((entry) => isPathWithin(path, entry.path))
}

export function addSelection(
  entries: readonly CollectorEntry[],
  node: TreeNodeSummary,
  scanId: string,
  addedAt: string,
  id: string,
): CollectorEntry[] {
  // A partial folder can contain inaccessible or excluded cloud children.
  if (node.scanState === 'skipped' || node.scanState === 'partial') return [...entries]
  if (selectedAncestor(entries, node.path)) return [...entries]
  return [
    ...entries.filter((entry) => !isPathWithin(entry.path, node.path)),
    { id, scanId, nodeId: node.id, path: node.path, name: node.name, size: node.size, addedAt },
  ]
}
