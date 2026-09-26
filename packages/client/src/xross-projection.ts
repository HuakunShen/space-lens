/** Presentational projection only. Opaque IDs remain the sole operation handles. */
import type { ScanStatus, TreeNodeSummary, TreeSlice, TreeSliceNode } from '@space-lens/contract'
import type { ScanStatusV1, SnapshotNodeV1, TreeSliceV1 } from '../../../integrations/xross/view-contract/surfaces/space-lens/api.js'

function count(value: string): number {
  const parsed = BigInt(value)
  return Number(parsed > BigInt(Number.MAX_SAFE_INTEGER) ? BigInt(Number.MAX_SAFE_INTEGER) : parsed)
}

export function projectScanStatus(scan: ScanStatusV1): ScanStatus {
  return {
    scanId: scan.scanId,
    state: scan.state === 'queued' ? 'scanning' : scan.state,
    message: '',
    progress: scan.progressPermille === undefined ? null : scan.progressPermille / 1000,
    // Native progress paths may be absolute; the peer UI only names approved roots.
    currentPath: null,
    bytesScanned: count(scan.bytesVisited),
    entriesScanned: count(scan.entriesVisited),
    rootIds: [...scan.rootIds],
    label: null,
    updatedAt: new Date(Number(scan.updatedAtUnixMs)).toISOString(),
  }
}

export function projectNode(node: SnapshotNodeV1, rootRelativePath = node.name): TreeNodeSummary {
  return {
    id: node.nodeId,
    name: node.name,
    path: rootRelativePath,
    size: count(node.sizeBytes),
    depth: node.depth,
    ignored: node.ignored,
    collapsed: node.collapsed,
    hasChildren: node.hasChildren,
    childCount: node.childCount,
  }
}

export function projectTreeSlice(scan: ScanStatusV1, slice: TreeSliceV1, rootLabel: string): TreeSlice {
  const ancestors = slice.ancestors.map((node, index) => {
    const path = index === 0 ? rootLabel : `${rootLabel}/${slice.ancestors.slice(1, index + 1).map((part) => part.name).join('/')}`
    return projectNode(node, path)
  })
  const focusPath = ancestors.length === 0 ? rootLabel : `${ancestors.at(-1)!.path}/${slice.tree.find((node) => node.nodeId === slice.focusNodeId)?.name ?? ''}`
  const sources = new Map(slice.tree.map((node) => [node.nodeId, node]))
  function pathFor(node: SnapshotNodeV1, seen = new Set<string>()): string {
    if (node.nodeId === slice.focusNodeId) return focusPath
    if (seen.has(node.nodeId)) throw new Error('tree slice contains cycle')
    seen.add(node.nodeId)
    const parent = node.parentId === undefined ? undefined : sources.get(node.parentId)
    if (parent === undefined) throw new Error('tree slice contains detached node')
    return `${pathFor(parent, seen)}/${node.name}`
  }
  const nodes = new Map<string, TreeSliceNode>()
  for (const source of slice.tree) {
    if (nodes.has(source.nodeId)) throw new Error('duplicate tree node')
    nodes.set(source.nodeId, { ...projectNode(source, pathFor(source)), children: [], omittedBytes: 0, omittedCount: 0 })
  }
  const focus = nodes.get(slice.focusNodeId)
  if (focus === undefined) throw new Error('focus node absent from tree slice')
  for (const source of slice.tree) {
    if (source.nodeId === slice.focusNodeId) continue
    const parent = source.parentId === undefined ? undefined : nodes.get(source.parentId)
    if (parent === undefined) throw new Error('tree slice contains detached node')
    parent.children.push(nodes.get(source.nodeId)!)
  }
  // The contract reports omission totals for the whole slice, not by parent.
  // Never fabricate a parent-specific "Other" wedge; the banner reports totals.
  return {
    scanId: scan.scanId,
    focusNode: projectNode(slice.tree.find((node) => node.nodeId === slice.focusNodeId)!, focusPath),
    ancestors,
    tree: focus,
    totalSize: count(slice.totalBytes),
    truncated: slice.truncated,
    omittedBytes: count(slice.omittedBytes),
    omittedCount: slice.omittedCount,
    generatedAt: new Date(Number(slice.generatedAtUnixMs)).toISOString(),
  }
}
