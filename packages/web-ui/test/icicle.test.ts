import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildIcicleSegments, clipIcicleName, icicleColumns, ICICLE_MAX_DEPTH } from '../src/lib/icicle.ts'
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

/**
 * The contract carries each node's absolute depth; a hand-built fixture has
 * to derive it the way a real scan does, or the icicle would paint columns
 * from structure while reporting depths from stale fields.
 */
function withDepths(current: TreeSliceNode, depth: number): TreeSliceNode {
  return { ...current, depth, children: current.children.map((child) => withDepths(child, depth + 1)) }
}

const WIDTH = 620
const HEIGHT = 430

test('columns are equal width and one per depth level', () => {
  const root = withDepths(node('root', 100, [node('a', 60, [node('a1', 60)]), node('b', 40)]), 0)
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT)
  const columns = icicleColumns(segments, WIDTH)
  assert.deepEqual(
    columns.map((column) => column.depth),
    [1, 2],
    'a two-level slice asks for two columns, not four empty ones',
  )
  const expected = WIDTH / ICICLE_MAX_DEPTH
  for (const column of columns) assert.ok(Math.abs(column.width - expected) < 1e-9)
  for (const segment of segments) assert.ok(Math.abs(segment.x1 - segment.x0 - expected) < 1e-9)
})

test('a segment spans its share of the parent, and siblings tile the parent', () => {
  const root = withDepths(node('root', 100, [node('big', 60), node('small', 20)]), 0)
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT)
  const big = segments.find((segment) => segment.id === 'big')!
  const small = segments.find((segment) => segment.id === 'small')!
  assert.ok(Math.abs(big.share - 0.75) < 1e-6)
  assert.ok(Math.abs(small.share - 0.25) < 1e-6)
  const bigSpan = big.y1 - big.y0
  const smallSpan = small.y1 - small.y0
  assert.ok(Math.abs(bigSpan / smallSpan - 3) < 0.02, `spans track the 3:1 ratio (got ${bigSpan} / ${smallSpan})`)
  assert.ok(Math.abs(bigSpan + smallSpan - HEIGHT) < 0.5, 'the column fills the canvas exactly')
  // Sorted biggest-first, so `big` sits above `small` with no gap between.
  assert.ok(Math.abs(big.y1 - small.y0) < 0.5)
})

test('children stay inside their parent span at every depth', () => {
  const root = withDepths(
    node('root', 100, [node('a', 70, [node('a1', 40), node('a2', 30)]), node('b', 30, [node('b1', 30)])]),
    0,
  )
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT)
  for (const parent of segments.filter((segment) => segment.depth === 1)) {
    const children = segments.filter(
      (segment) => segment.depth === 2 && segment.y0 >= parent.y0 - 0.01 && segment.y1 <= parent.y1 + 0.01,
    )
    assert.equal(children.length, parent.node.children.length, `${parent.name} keeps its children inside`)
    for (const child of children) assert.equal(child.x0, parent.x1, 'a child starts where its parent ends')
  }
})

test('deep trees keep equal columns instead of compressing them', () => {
  // d3's partition scales its depth axis by the hierarchy's own height, so a
  // tree deeper than the cap used to squeeze every column. Columns come from
  // `depth` now; this is the regression guard.
  let deep = node('leaf', 10)
  for (let level = 8; level >= 1; level -= 1) deep = node(`n${level}`, 10, [deep])
  const root = withDepths(node('root', 10, [deep]), 0)
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT)
  assert.equal(segments.length, ICICLE_MAX_DEPTH, 'depth is capped, not overrun')
  for (const segment of segments) {
    assert.ok(Math.abs(segment.x1 - segment.x0 - WIDTH / ICICLE_MAX_DEPTH) < 1e-9)
    assert.ok(segment.y0 >= -0.01 && segment.y1 <= HEIGHT + 0.01)
  }
})

test('omitted buckets surface as an Other segment exactly once', () => {
  const folder = node('folder', 80, [], { hasChildren: true, childCount: 8, omittedBytes: 80, omittedCount: 8 })
  const root = withDepths(node('root', 100, [folder, node('file', 20)], { omittedBytes: 80, omittedCount: 8 }), 0)
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT, 3)
  const aggregates = segments.filter((segment) => segment.isAggregate)
  assert.deepEqual(
    aggregates.map((segment) => [segment.id, segment.size]),
    [['folder:omitted', 80]],
  )
  assert.equal(aggregates[0]!.depth, 2, 'the bucket hangs under its own parent')
})

test('degenerate inputs never produce NaN geometry', () => {
  assert.deepEqual(buildIcicleSegments(node('empty', 0), WIDTH, HEIGHT), [])
  assert.deepEqual(buildIcicleSegments(node('root', 10, [node('a', 10)]), 0, HEIGHT), [])
  assert.deepEqual(buildIcicleSegments(node('root', 10, [node('a', 10)]), WIDTH, 0), [])
  assert.deepEqual(buildIcicleSegments(node('root', 10, [node('a', 10)]), WIDTH, HEIGHT, 0), [])
  for (const segment of buildIcicleSegments(node('root', 0, [node('zero', 0)]), WIDTH, HEIGHT)) {
    for (const value of [segment.x0, segment.y0, segment.x1, segment.y1, segment.share]) {
      assert.ok(Number.isFinite(value), `finite geometry, got ${value}`)
    }
  }
})

test('labels are withheld when the segment has no room', () => {
  const root = withDepths(node('root', 100, [node('wide', 99), node('sliver', 1)]), 0)
  const segments = buildIcicleSegments(root, WIDTH, HEIGHT)
  assert.equal(segments.find((segment) => segment.id === 'wide')!.labelVisible, true)
  assert.equal(segments.find((segment) => segment.id === 'sliver')!.labelVisible, false)
  assert.equal(clipIcicleName('src', 200), 'src')
  assert.equal(clipIcicleName('node_modules', 40).endsWith('…'), true)
  assert.equal(clipIcicleName('anything', 8), '')
})

test('the canvas height decides how tall every row is', () => {
  // This is what fixed the unreadable slivers: rows follow the stage, so a
  // bigger workbench buys real label room instead of scaling a fixed canvas.
  const root = withDepths(node('root', 100, [node('a', 60), node('b', 40)]), 0)
  const spanOf = (height: number, id: string) => {
    const segment = buildIcicleSegments(root, 620, height).find((s) => s.id === id)!
    return segment.y1 - segment.y0
  }
  assert.ok(spanOf(600, 'a') > spanOf(200, 'a') * 2.5)
  assert.ok(Math.abs(spanOf(600, 'a') + spanOf(600, 'b') - 600) < 0.5)
})
