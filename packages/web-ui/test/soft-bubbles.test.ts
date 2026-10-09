/** Node invariants for the pure soft-bubble solver, using the production packed layout. */
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildBubbleCircles, type BubbleCircle } from '../src/lib/bubbles.ts'
import { area, centroid, circlePoints, inside, nearest, reach, resample } from '../src/lib/soft-bubbles/geometry.ts'
import {
  MAX_MEMBRANES,
  PARENT_VERTEX_COUNT,
  CHILD_VERTEX_COUNT,
  MIN_MEMBRANE_RADIUS,
  SoftBubbleWorld,
  bubbleAtPoint,
  type SoftBubbleSnapshot,
} from '../src/lib/soft-bubbles/world.ts'
import type { TreeSliceNode } from '../src/types.ts'

function node(id: string, children: TreeSliceNode[] = [], size = 100): TreeSliceNode {
  return {
    id,
    name: id,
    path: `/root/${id}`,
    size: children.length ? children.reduce((sum, child) => sum + child.size, 0) : size,
    depth: 0,
    ignored: false,
    collapsed: false,
    hasChildren: children.length > 0,
    childCount: children.length,
    children,
    omittedBytes: 0,
    omittedCount: 0,
  }
}
function withDepths(current: TreeSliceNode, depth: number): TreeSliceNode {
  return { ...current, depth, children: current.children.map((child) => withDepths(child, depth + 1)) }
}
function ordinary(): BubbleCircle[] {
  const root = node(
    'root',
    Array.from({ length: 6 }, (_, parent) =>
      node(
        `p${parent}`,
        Array.from({ length: 4 }, (_, child) => node(`p${parent}/c${child}`, [], 150 + parent * 20 + child * 40)),
      ),
    ),
  )
  return buildBubbleCircles(withDepths(root, 7), 1000, 760).filter((circle) => circle.id !== 'root')
}
function largeDirectory(): BubbleCircle[] {
  const children = Array.from({ length: 300 }, (_, index) => node(`node_modules/${index}`, [], 100 + (index % 5)))
  return buildBubbleCircles(withDepths(node('root', [node('node_modules', children)]), 4), 860, 640).filter(
    (circle) => circle.id !== 'root',
  )
}
function getBody(snapshot: SoftBubbleSnapshot[], id: string): SoftBubbleSnapshot {
  const body = snapshot.find((candidate) => candidate.id === id)
  assert.ok(body, `body ${id} exists`)
  return body
}
function assertState(snapshot: SoftBubbleSnapshot[]): void {
  const byId = new Map(snapshot.map((body) => [body.id, body]))
  for (const body of snapshot) {
    assert.ok(Number.isFinite(body.cx) && Number.isFinite(body.cy))
    assert.ok(
      body.points.every((point) => Number.isFinite(point.x) && Number.isFinite(point.y)),
      `${body.id} is finite`,
    )
    assert.ok(area(body.points) > 0, `${body.id} has positive area`)
    if (body.parentId && body.kind === 'membrane') {
      const parent = byId.get(body.parentId)
      assert.ok(parent)
      const contained = body.points.reduce((count, point) => count + Number(inside(point, parent.points)), 0)
      assert.ok(
        contained / body.points.length >= 0.9,
        `${body.id}: ${contained}/${body.points.length} vertices in ${parent.id}`,
      )
    }
  }
}

test('geometry retains angular ordering, bounding reach, and outward edge normals', () => {
  const points = circlePoints(20, 30, 10, 64)
  assert.ok(area(points) > 300)
  assert.ok(Math.hypot(centroid(points).x - 20, centroid(points).y - 30) < 1e-12)
  assert.ok(Math.abs(reach(points).radius - 10) < 1e-12)
  assert.equal(inside({ x: 20, y: 30 }, points), true)
  assert.equal(inside({ x: 40, y: 30 }, points), false)
  const hit = nearest({ x: 40, y: 30 }, points)
  assert.ok(Math.abs(hit.distance - 10) < 1e-12 && hit.nx > 0.99)
  assert.deepEqual(resample(points, 64), points)
  assert.deepEqual(resample([], 64), [])
})

for (const [name, fixture] of [
  ['ordinary 6 × 4 tree', ordinary],
  ['300-grandchild node_modules tree', largeDirectory],
] satisfies [string, () => BubbleCircle[]][]) {
  test(`${name}: initial contours retain canonical POC particles and sleeping steps never drift`, () => {
    const circles = fixture()
    const world = new SoftBubbleWorld(circles, 6)
    const seed = world.snapshot()
    assert.equal(seed.length, circles.length)
    for (const body of seed)
      assert.deepEqual(
        body.points,
        resample(
          circlePoints(
            body.circle.x + 6,
            body.circle.y + 6,
            body.r,
            body.kind === 'passenger' ? 64 : body.parentId ? CHILD_VERTEX_COUNT : PARENT_VERTEX_COUNT,
          ),
          64,
        ),
      )
    for (let step = 0; step < 300; step += 1) world.step()
    assert.deepEqual(world.snapshot(), seed)
    assert.equal(world.isAtRest(), true)
    assertState(seed)
    for (const body of world.snapshot(128)) {
      assert.equal(body.points.length, 128)
      assert.deepEqual(
        body.points,
        resample(
          circlePoints(
            body.circle.x + 6,
            body.circle.y + 6,
            body.r,
            body.kind === 'passenger' ? 128 : body.parentId ? CHILD_VERTEX_COUNT : PARENT_VERTEX_COUNT,
          ),
          128,
        ),
      )
    }
  })
}

for (const id of ['p0', 'p0/c0']) {
  test(`${id}: drag, finite contained rebound, and return within 600 steps`, () => {
    const world = new SoftBubbleWorld(ordinary(), 6)
    const seed = world.snapshot()
    const body = getBody(seed, id)
    assert.equal(world.drag(id, { x: body.cx + 150, y: body.cy }), true)
    for (let step = 0; step < 60; step += 1) {
      world.step()
      assertState(world.snapshot())
    }
    const moved = getBody(world.snapshot(), id)
    assert.ok(
      Math.hypot(moved.cx - body.cx, moved.cy - body.cy) > 5,
      'the active solver actually moves the drag target',
    )
    world.release()
    let steps = 0
    while (!world.isAtRest() && steps < 600) {
      world.step()
      assertState(world.snapshot())
      steps += 1
    }
    assert.ok(world.isAtRest(), `returned after ${steps} steps`)
    for (const returned of world.snapshot()) {
      const original = getBody(seed, returned.id)
      for (let index = 0; index < returned.points.length; index += 1) {
        assert.ok(
          Math.hypot(
            returned.points[index].x - original.points[index].x,
            returned.points[index].y - original.points[index].y,
          ) < 0.5,
        )
      }
    }
    for (let step = 0; step < 300; step += 1) world.step()
    assert.deepEqual(world.snapshot(), seed)
  })
}

test('the body budget keeps hundreds of rigid passengers finite while their parent is dragged', () => {
  const circles = largeDirectory()
  assert.ok(circles.length >= 300, 'the realistic layout retains the 300 visible grandchildren')
  const world = new SoftBubbleWorld(circles)
  const snapshot = world.snapshot()
  const membranes = snapshot.filter((body) => body.kind === 'membrane')
  const passengers = snapshot.filter((body) => body.kind === 'passenger')
  assert.equal(MAX_MEMBRANES, 64)
  assert.equal(MIN_MEMBRANE_RADIUS, 14)
  assert.ok(membranes.length <= MAX_MEMBRANES)
  assert.ok(passengers.length > 0)
  assert.ok(passengers.every((body) => body.parentId === 'node_modules'))
  assert.equal(world.drag(passengers[0].id, { x: 0, y: 0 }), false)
  const parent = getBody(snapshot, 'node_modules')
  world.drag(parent.id, { x: parent.cx + 150, y: parent.cy + 60 })
  for (let step = 0; step < 60; step += 1) {
    world.step()
    assertState(world.snapshot())
  }
  world.release()
  for (let step = 0; step < 600 && !world.isAtRest(); step += 1) {
    world.step()
    assertState(world.snapshot())
  }
  assert.equal(world.isAtRest(), true)
})

test('largest second-level circles receive the remaining membrane budget', () => {
  const children = Array.from({ length: 100 }, (_, index) => node(`child${index}`, [], 100 + index))
  const circles = buildBubbleCircles(withDepths(node('root', [node('folder', children)]), 0), 2000, 1800).filter(
    (circle) => circle.id !== 'root',
  )
  const snapshot = new SoftBubbleWorld(circles).snapshot()
  const membranes = snapshot.filter((body) => body.kind === 'membrane')
  assert.equal(membranes.length, MAX_MEMBRANES)
  const smallestMembrane = Math.min(...membranes.filter((body) => body.parentId).map((body) => body.r))
  assert.ok(snapshot.filter((body) => body.kind === 'passenger').every((body) => body.r <= smallestMembrane))
})

test('aggregates participate without being draggable; invalid requests do not activate the world', () => {
  const circles = ordinary()
  circles[0].isAggregate = true
  const world = new SoftBubbleWorld(circles)
  assert.equal(world.drag(circles[0].id, { x: 10, y: 20 }), false)
  assert.equal(world.drag('missing', { x: 0, y: 0 }), false)
  assert.equal(world.drag(circles[1].id, { x: NaN, y: 0 }), false)
  assert.equal(world.isAtRest(), true)
  assert.deepEqual(new SoftBubbleWorld([]).snapshot(), [])
})

test('a drag released before the first animation frame still deforms and rebounds', () => {
  const world = new SoftBubbleWorld(ordinary(), 6)
  const seed = world.snapshot()
  const body = getBody(seed, 'p0')
  world.drag('p0', { x: body.cx + 150, y: body.cy })
  world.release()
  assert.equal(world.isAtRest(), false, 'release must not discard an unconsumed drag target')
  assertState(world.snapshot())
  for (let step = 0; step < 600 && !world.isAtRest(); step += 1) world.step()
  assert.equal(world.isAtRest(), true)
})

test('compressed equilibrium preserves an exact handoff and sleeps in its actual settled pose', () => {
  const circles = ordinary()
  const settled = new SoftBubbleWorld(circles, 6).compressedRest(0.55)
  assertState(settled)
  const parents = settled.filter((body) => !body.parentId)
  assert.ok(
    parents.some((body) => {
      const radii = body.points.map((p) => Math.hypot(p.x - body.cx, p.y - body.cy))
      return Math.max(...radii) - Math.min(...radii) > 4
    }),
    'contact should visibly flatten a resting parent, not just move a circle',
  )
  const world = new SoftBubbleWorld(circles, 6, settled)
  assert.deepEqual(world.snapshot(), settled, 'static-to-Canvas handoff is exact')
  for (let i = 0; i < 300; i++) world.step()
  assert.deepEqual(world.snapshot(), settled, 'no idle simulation or drift')
  const body = getBody(settled, 'p0')
  world.drag(body.id, { x: body.cx + 110, y: body.cy + 20 })
  for (let i = 0; i < 45; i++) world.step()
  world.release()
  for (let i = 0; i < 600 && !world.isAtRest(); i++) world.step()
  assert.equal(world.isAtRest(), true)
  const final = world.snapshot()
  for (let i = 0; i < 300; i++) world.step()
  assert.deepEqual(world.snapshot(), final, 'settled contours never shimmer or run idle physics')
  assert.deepEqual(
    new SoftBubbleWorld(circles, 6, final).snapshot(),
    final,
    'next gesture keeps the same canonical particle state',
  )
})

function touchingPair(): BubbleCircle[] {
  return [200, 365].map((x, i) => ({
    id: `pair-${i}`,
    name: `pair-${i}`,
    path: `/pair-${i}`,
    size: 100,
    depth: 1,
    childCount: 0,
    share: 0.5,
    color: '#6e40b8',
    x,
    y: 200,
    r: 80,
    labelVisible: true,
    isAggregate: false,
    hasChildren: false,
    node: node(`pair-${i}`),
  }))
}
function radialVariation(body: SoftBubbleSnapshot): number {
  const radii = body.points.map((point) => Math.hypot(point.x - body.cx, point.y - body.cy))
  return Math.max(...radii) - Math.min(...radii)
}

test('a squeezed bubble rounds out when pulled clear of its neighbor, like the POC membrane', () => {
  const circles = touchingPair()
  const resting = new SoftBubbleWorld(circles).compressedRest()
  const initial = resting[0]
  assert.ok(radialVariation(initial) > 6)
  const world = new SoftBubbleWorld(circles, 0, resting)
  world.drag(initial.id, { x: initial.cx - 160, y: initial.cy })
  for (let step = 0; step < 180; step++) world.step()
  const pulled = world.snapshot()[0]
  assert.ok(Math.abs(pulled.cx - initial.cx) > 100)
  assert.ok(
    radialVariation(pulled) < radialVariation(initial) / 3,
    'a contact patch must relax in free space instead of being baked into the spring shape',
  )
})

test('the first pointer press targets the visible child; Alt targets its parent without an ECharts mouse event', () => {
  const scene = new SoftBubbleWorld(ordinary(), 6).compressedRest()
  const child = getBody(scene, 'p0/c0')
  const point = { x: child.cx, y: child.cy }
  assert.equal(bubbleAtPoint(scene, point)?.id, child.id)
  assert.equal(bubbleAtPoint(scene, point, true)?.id, 'p0')
  assert.equal(bubbleAtPoint(scene, { x: -500, y: -500 }), undefined)
})
