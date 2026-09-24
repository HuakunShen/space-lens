import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildSunburstSegments } from '../src/lib/sunburst.ts'
import { nodeColor, nodeMutedColor } from '../src/lib/colors.ts'
import type { TreeSliceNode } from '../src/types.ts'

function node(
  id: string,
  size: number,
  children: TreeSliceNode[] = [],
  extra: Partial<TreeSliceNode> = {},
): TreeSliceNode {
  return {
    id,
    name: id,
    path: `/root/${id}`,
    size,
    depth: 1,
    ignored: false,
    collapsed: false,
    hasChildren: children.length > 0,
    childCount: children.length,
    children,
    omittedBytes: 0,
    omittedCount: 0,
    ...extra,
  }
}

test('deep truncation is represented once, without inflating parent wedges', () => {
  const folder = node('folder', 80, [], { hasChildren: true, childCount: 8, omittedBytes: 80, omittedCount: 8 })
  const root = node('root', 100, [folder, node('file', 20)], { depth: 0, omittedBytes: 80, omittedCount: 8 })
  const segments = buildSunburstSegments(root, 285)
  assert.deepEqual(
    segments.filter((s) => s.isAggregate).map((s) => [s.id, s.size]),
    [['folder:omitted', 80]],
  )
  const complete = buildSunburstSegments(node('root', 100, [node('folder', 80), node('file', 20)], { depth: 0 }), 285)
  // A deeper ring must not change the angular midpoint of an existing 80% wedge.
  const angle = (s: (typeof segments)[number]) => Math.atan2(s.labelY, s.labelX)
  assert.ok(
    Math.abs(angle(segments.find((s) => s.id === 'folder')!) - angle(complete.find((s) => s.id === 'folder')!)) < 1e-10,
  )
})

test('direct and nested omissions retain their own sizes and counts', () => {
  const child = node('folder', 60, [], { hasChildren: true, childCount: 6, omittedBytes: 60, omittedCount: 6 })
  const segments = buildSunburstSegments(node('root', 100, [child], { omittedBytes: 100, omittedCount: 10 }), 285)
  const other = segments.find((s) => s.id === 'root:omitted')!
  assert.equal(other.size, 40)
  assert.equal(other.childCount, 4)
  assert.equal(segments.find((s) => s.id === 'folder:omitted')!.size, 60)
})

test('chart and list colors agree after drilling into a deep folder', () => {
  const child = node('deep-file', 20, [], { depth: 8 })
  const ignored = node('ignored', 10, [], { depth: 8, ignored: true })
  const segments = buildSunburstSegments(node('folder', 30, [child, ignored], { depth: 7 }), 285)
  assert.equal(segments.find((s) => s.id === child.id)!.color, nodeColor(child.id, child.depth))
  assert.equal(segments.find((s) => s.id === ignored.id)!.color, nodeMutedColor(ignored.depth))
})

test('empty folders and zero-byte items never produce invalid paths', () => {
  assert.deepEqual(buildSunburstSegments(node('empty', 0), 285), [])
  for (const segment of buildSunburstSegments(node('root', 0, [node('zero', 0)]), 285)) {
    assert.ok(segment.pathData.length > 0)
    assert.doesNotMatch(segment.pathData, /NaN|Infinity/)
  }
})
