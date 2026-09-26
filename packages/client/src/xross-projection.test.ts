import { describe, expect, it } from 'vitest'
import { projectNode, projectScanStatus, projectTreeSlice } from './xross-projection.js'
import type { ScanStatusV1, SnapshotNodeV1, TreeSliceV1 } from '../../../integrations/xross/view-contract/surfaces/space-lens/api.js'

const scan = {
  scanId: `xscan_${'1'.repeat(32)}`, state: 'ready', rootIds: [`xroot_${'1'.repeat(32)}`],
  bytesVisited: '500', entriesVisited: '3', updatedAtUnixMs: '1735689600000',
} as unknown as ScanStatusV1
const root = {
  nodeId: `xnode_${'1'.repeat(32)}`, name: 'Storage', displayPath: 'Storage',
  sizeBytes: '500', depth: 0, childCount: 2, ignored: false, collapsed: false, hasChildren: true,
} as SnapshotNodeV1
const child = {
  nodeId: `xnode_${'2'.repeat(32)}`, parentId: root.nodeId, name: 'A', displayPath: '/Users/private/A',
  sizeBytes: '200', depth: 1, childCount: 0, ignored: false, collapsed: false, hasChildren: false,
} as SnapshotNodeV1

describe('Xross presentational projection', () => {
  it('keeps node handles distinct from roots and projects bounded display data', () => {
    expect(projectScanStatus(scan).rootIds).toEqual(scan.rootIds)
    expect(projectNode(child, 'Storage/A')).toMatchObject({ id: child.nodeId, path: 'Storage/A', size: 200 })
  })

  it('links a flat slice without inventing parent-specific omission accounting', () => {
    const source = {
      kind: 'treeSlice', snapshotId: `xsnapshot_${'1'.repeat(32)}`, focusNodeId: root.nodeId,
      tree: [child, root], ancestors: [], totalBytes: '500', truncated: true,
      omittedBytes: '300', omittedCount: 1, generatedAtUnixMs: '1735689600000',
    } as unknown as TreeSliceV1
    const output = projectTreeSlice(scan, source, 'Storage')
    expect(output.tree.children.map((node) => node.id)).toEqual([child.nodeId])
    expect(output.tree.children[0]?.path).toBe('Storage/A')
    expect(output.tree.omittedBytes).toBe(0)
    expect(output.omittedBytes).toBe(300)
    expect(output.tree.id).not.toBe(scan.rootIds[0])
  })

  it('refuses detached or duplicate nodes instead of rendering a misleading tree', () => {
    const base = {
      kind: 'treeSlice', snapshotId: `xsnapshot_${'1'.repeat(32)}`, focusNodeId: root.nodeId,
      ancestors: [], totalBytes: '500', truncated: false, omittedBytes: '0', omittedCount: 0,
      generatedAtUnixMs: '1735689600000',
    } as const
    expect(() => projectTreeSlice(scan, { ...base, tree: [root, root] } as unknown as TreeSliceV1, 'Storage')).toThrow('duplicate')
    expect(() => projectTreeSlice(scan, { ...base, tree: [child, root] } as unknown as TreeSliceV1, 'Storage')).not.toThrow()
    const orphan = { ...child, parentId: `xnode_${'3'.repeat(32)}` } as SnapshotNodeV1
    expect(() => projectTreeSlice(scan, { ...base, tree: [orphan, root] } as unknown as TreeSliceV1, 'Storage')).toThrow('detached')
  })
})
