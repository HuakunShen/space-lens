import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildStripRows, rankLargestItems } from '../src/lib/strips.ts'
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

/** Mirrors a real scan: every node carries its absolute depth. */
function withDepths(current: TreeSliceNode, depth: number): TreeSliceNode {
  return { ...current, depth, children: current.children.map((child) => withDepths(child, depth + 1)) }
}

const round = (value: number) => Math.round(value * 1e6) / 1e6

test('rows are ranked biggest first and their shares sum to one', () => {
  const root = withDepths(node('root', 100, [node('small', 20), node('big', 60), node('mid', 20)]), 0)
  const rows = buildStripRows(root)
  assert.deepEqual(
    rows.map((row) => row.id),
    ['big', 'small', 'mid'],
    'biggest first, ties keeping their scan order',
  )
  assert.ok(Math.abs(rows.reduce((sum, row) => sum + row.share, 0) - 1) < 1e-9)
  assert.ok(Math.abs(rows[0]!.share - 0.6) < 1e-9)
})

test('a row is weighted by leaf size, like the other four charts', () => {
  // `folder` measures 90 but holds only a 10-byte leaf, so it must not
  // outrank `file`'s honest 20 bytes. Weighting by measured size would put
  // folder first and disagree with the sunburst for the same slice.
  const root = withDepths(node('root', 110, [node('folder', 90, [node('tiny', 10)]), node('file', 20)]), 0)
  const rows = buildStripRows(root)
  assert.deepEqual(
    rows.map((row) => row.id),
    ['file', 'folder'],
  )
  assert.ok(Math.abs(rows[0]!.share - 20 / 30) < 1e-9)
  assert.equal(rows[1]!.size, 90, 'the measured size is still reported for display')
})

test('a row splits into its children, filling the row exactly', () => {
  const root = withDepths(node('root', 100, [node('a', 60, [node('a1', 40), node('a2', 20)]), node('b', 40)]), 0)
  const rows = buildStripRows(root)
  const a = rows.find((row) => row.id === 'a')!
  assert.deepEqual(
    a.segments.map((segment) => segment.id),
    ['a1', 'a2'],
  )
  assert.ok(Math.abs(a.segments.reduce((sum, segment) => sum + segment.share, 0) - 1) < 1e-9)
  assert.ok(Math.abs(a.segments[0]!.share - 2 / 3) < 1e-9)
  // A row with nothing deeper renders as one solid bar, not a zero-width split.
  assert.deepEqual(rows.find((row) => row.id === 'b')!.segments, [])
})

test('segments inherit their row family and ignored entries stay grey', () => {
  const quiet = node('quiet', 10, [], { ignored: true })
  const root = withDepths(node('root', 100, [node('loud', 70, [node('loud-inner', 70)]), quiet]), 0)
  const rows = buildStripRows(root)
  const loud = rows.find((row) => row.id === 'loud')!
  assert.equal(loud.color, nodeColor('loud', 1, 'loud'))
  // The segment borrows the row's id as its hue family.
  assert.equal(loud.segments[0]!.color, nodeColor('loud-inner', 2, 'loud'))
  assert.equal(rows.find((row) => row.id === 'quiet')!.color, nodeMutedColor(1))
})

test('omitted buckets surface once, as a segment of the folder that owns them', () => {
  const folder = node('folder', 80, [], { hasChildren: true, childCount: 8, omittedBytes: 80, omittedCount: 8 })
  const root = withDepths(node('root', 100, [folder, node('file', 20)], { omittedBytes: 80, omittedCount: 8 }), 0)
  const rows = buildStripRows(root)
  // The root's own omission total is already spent by `folder`'s, exactly as
  // the sunburst's `withOmittedBuckets` contract requires — so the bucket
  // hangs under `folder` and shows up as that row's only segment.
  assert.deepEqual(
    rows.map((row) => row.id),
    ['folder', 'file'],
  )
  const segments = rows.flatMap((row) => row.segments)
  assert.deepEqual(
    segments.map((segment) => [segment.id, segment.size]),
    [['folder:omitted', 80]],
  )
  assert.equal(segments[0]!.isAggregate, true, 'the bucket is flagged so the inspector can say so')
  assert.equal(
    rows.every((row) => !row.isAggregate),
    true,
    'neither real row is a bucket',
  )
})

test('the largest-items rail ranks one level below the rows', () => {
  const root = withDepths(
    node('root', 200, [
      node('Applications', 120, [node('Xcode', 26), node('Docker', 6)]),
      node('Users', 80, [node('Photos', 18)]),
    ]),
    0,
  )
  const ranked = rankLargestItems(root, 3)
  assert.deepEqual(
    ranked.map((item) => item.name),
    ['Xcode', 'Photos', 'Docker'],
    'the folders inside the biggest folders, not the rows again',
  )
  assert.ok(Math.abs(ranked[0]!.share - 26 / 200) < 1e-9, 'share is of the focused folder, not the row')
  // A slice with nothing deeper falls back to the rows rather than empty.
  const flat = withDepths(node('root', 100, [node('a', 60), node('b', 40)]), 0)
  assert.deepEqual(
    rankLargestItems(flat, 5).map((item) => item.name),
    ['a', 'b'],
  )
  assert.deepEqual(rankLargestItems(root, 0), [])
})

test('degenerate inputs return no rows rather than throwing', () => {
  assert.deepEqual(buildStripRows(node('empty', 0)), [], 'a folder with no children has nothing to rank')
  // A zero-byte child still earns a row, matching the sunburst and treemap,
  // which both give a zero-size leaf a hairline rather than dropping it. The
  // row prints "0 B", so the degenerate 100% share cannot mislead.
  const zero = buildStripRows(withDepths(node('root', 0, [node('zero', 0)]), 0))
  assert.deepEqual(
    zero.map((row) => row.id),
    ['zero'],
  )
  assert.ok(Number.isFinite(zero[0]!.share))
  const rows = buildStripRows(withDepths(node('root', 100, [node('a', 100)]), 0))
  assert.equal(round(rows[0]!.share), 1)
})
