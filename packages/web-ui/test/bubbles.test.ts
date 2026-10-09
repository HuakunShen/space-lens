import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildBubbleCircles, clipBubbleName, BUBBLE_MAX_DEPTH } from '../src/lib/bubbles.ts'
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

const WIDTH = 620
const HEIGHT = 430
const area = (radius: number) => Math.PI * radius * radius

test('circle area, not radius, tracks size', () => {
  const root = withDepths(node('root', 100, [node('big', 60), node('small', 20)]), 0)
  const circles = buildBubbleCircles(root, WIDTH, HEIGHT)
  const big = circles.find((circle) => circle.id === 'big')!
  const small = circles.find((circle) => circle.id === 'small')!
  assert.ok(Math.abs(big.share - 0.75) < 1e-6)
  assert.ok(Math.abs(area(big.r) / area(small.r) - 3) < 0.02, 'a 3:1 size gap is a 3:1 ink gap')
  // Radius alone would be sqrt(3) ≈ 1.73; confirm we did not ship that.
  assert.ok(Math.abs(big.r / small.r - Math.sqrt(3)) < 0.02)
})

test('circles at the same depth never overlap', () => {
  const kids = Array.from({ length: 12 }, (_, index) =>
    node(`k${String(index).padStart(2, '0')}`, (index + 1) * 10, [node(`k${index}-inner`, 5)]),
  )
  const circles = buildBubbleCircles(withDepths(node('root', 780, kids), 0), WIDTH, HEIGHT)
  for (const depth of [1, 2]) {
    const level = circles.filter((circle) => circle.depth === depth)
    for (let a = 0; a < level.length; a += 1) {
      for (let b = a + 1; b < level.length; b += 1) {
        const left = level[a]!
        const right = level[b]!
        const distance = Math.hypot(left.x - right.x, left.y - right.y)
        assert.ok(
          distance >= left.r + right.r - 0.5,
          `${left.id} overlaps ${right.id} (gap ${distance - left.r - right.r})`,
        )
      }
    }
  }
})

test('every circle stays inside the canvas and keeps its parent', () => {
  const root = withDepths(
    node('root', 100, [node('a', 70, [node('a1', 40), node('a2', 30)]), node('b', 30, [node('b1', 30)])]),
    0,
  )
  const circles = buildBubbleCircles(root, WIDTH, HEIGHT)
  for (const circle of circles) {
    assert.ok(circle.x - circle.r >= -0.01 && circle.x + circle.r <= WIDTH + 0.01, `${circle.id} spills horizontally`)
    assert.ok(circle.y - circle.r >= -0.01 && circle.y + circle.r <= HEIGHT + 0.01, `${circle.id} spills vertically`)
    assert.ok(circle.r > 0)
  }
  // A child never escapes the circle that contains it.
  const a = circles.find((circle) => circle.id === 'a')!
  for (const id of ['a1', 'a2']) {
    const child = circles.find((circle) => circle.id === id)!
    assert.ok(Math.hypot(child.x - a.x, child.y - a.y) + child.r <= a.r + 0.01, `${id} escapes its parent`)
  }
})

test('the pack is deterministic across runs', () => {
  const root = withDepths(node('root', 100, [node('a', 50), node('b', 30), node('c', 20)]), 0)
  const first = buildBubbleCircles(root, WIDTH, HEIGHT)
  const second = buildBubbleCircles(root, WIDTH, HEIGHT)
  assert.deepEqual(
    first.map((circle) => [circle.id, circle.x, circle.y, circle.r]),
    second.map((circle) => [circle.id, circle.x, circle.y, circle.r]),
  )
})

test('roomy parent bubbles reserve a clear title band above their children', () => {
  const root = withDepths(node('root', 100, [node('folder', 100, [node('a', 100)])]), 0)
  const circles = buildBubbleCircles(root, WIDTH, HEIGHT)
  const parent = circles.find((circle) => circle.id === 'folder')!
  const children = circles.filter((circle) => circle.depth === 2)
  const top = Math.min(...children.map((circle) => circle.y - circle.r))
  assert.ok(top - (parent.y - parent.r) >= 48, 'the name and size have room above the child pack')
  for (const child of children) {
    assert.ok(Math.hypot(child.x - parent.x, child.y - parent.y) + child.r <= parent.r)
  }
})

test('equal-sized child bubbles keep equal areas across differently sized parents', () => {
  const root = withDepths(
    node('root', 101, [
      node('big-folder', 100, [node('small-child', 1), node('large-child', 99)]),
      node('small-folder', 1, [node('equal-child', 1)]),
    ]),
    0,
  )
  const circles = buildBubbleCircles(root, WIDTH, HEIGHT)
  const left = circles.find((circle) => circle.id === 'small-child')!
  const right = circles.find((circle) => circle.id === 'equal-child')!
  assert.ok(left && right, 'both measured children remain visible')
  assert.ok(Math.abs(area(left.r) / area(right.r) - 1) < 0.02, 'equal sizes get equal ink at the same depth')
})

test('depth is capped and the root stays available as a backdrop', () => {
  let deep = node('leaf', 10)
  for (let level = 5; level >= 1; level -= 1) deep = node(`n${level}`, 10, [deep])
  const circles = buildBubbleCircles(withDepths(node('root', 10, [deep]), 0), WIDTH, HEIGHT)
  assert.ok(
    circles.some((circle) => circle.depth === 0),
    'depth 0 is kept for the backdrop',
  )
  assert.ok(circles.every((circle) => circle.depth <= BUBBLE_MAX_DEPTH))
  // The hierarchy is deeper than the cap, so depth 2 must still be painted.
  assert.ok(circles.some((circle) => circle.depth === 2))
})

test('omitted buckets surface as an Other circle exactly once', () => {
  const folder = node('folder', 80, [], { hasChildren: true, childCount: 8, omittedBytes: 80, omittedCount: 8 })
  const root = withDepths(node('root', 100, [folder, node('file', 20)], { omittedBytes: 80, omittedCount: 8 }), 0)
  const aggregates = buildBubbleCircles(root, WIDTH, HEIGHT).filter((circle) => circle.isAggregate)
  assert.deepEqual(
    aggregates.map((circle) => [circle.id, circle.size]),
    [['folder:omitted', 80]],
  )
})

test('degenerate inputs never produce NaN geometry', () => {
  assert.deepEqual(buildBubbleCircles(node('empty', 0), WIDTH, HEIGHT), [])
  assert.deepEqual(buildBubbleCircles(node('root', 10, [node('a', 10)]), 0, HEIGHT), [])
  assert.deepEqual(buildBubbleCircles(node('root', 10, [node('a', 10)]), WIDTH, 0), [])
  for (const circle of buildBubbleCircles(withDepths(node('root', 0, [node('zero', 0)]), 0), WIDTH, HEIGHT)) {
    assert.ok(Number.isFinite(circle.x) && Number.isFinite(circle.y) && Number.isFinite(circle.r))
  }
})

test('labels are withheld from circles too small to hold them', () => {
  const root = withDepths(node('root', 100, [node('wide', 99), node('sliver', 1)]), 0)
  const circles = buildBubbleCircles(root, WIDTH, HEIGHT)
  assert.equal(circles.find((circle) => circle.id === 'wide')!.labelVisible, true)
  assert.equal(circles.find((circle) => circle.id === 'sliver')!.labelVisible, false)
  assert.equal(clipBubbleName('src', 120), 'src')
  assert.equal(clipBubbleName('node_modules', 20).endsWith('…'), true)
  assert.equal(clipBubbleName('anything', 4), '')
})
