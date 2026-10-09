/** Standalone active-drag benchmark using production d3 packing; run with modern Node. */
import { performance } from 'node:perf_hooks'
import { buildBubbleCircles } from '../src/lib/bubbles.ts'
import { SoftBubbleWorld } from '../src/lib/soft-bubbles/world.ts'
import type { TreeSliceNode } from '../src/types.ts'

function node(id: string, depth: number, children: TreeSliceNode[] = [], size = 100): TreeSliceNode {
  return {
    id,
    name: id,
    path: `/root/${id}`,
    depth,
    size: children.length ? children.reduce((sum, child) => sum + child.size, 0) : size,
    ignored: false,
    collapsed: false,
    hasChildren: children.length > 0,
    childCount: children.length,
    children,
    omittedBytes: 0,
    omittedCount: 0,
  }
}

console.log('soft bubbles: production circle layouts, 30 warm-up + 120 measured ACTIVE drag steps')
for (const [parentCount, childCount] of [
  [6, 4],
  [12, 6],
  [24, 8],
  [40, 10],
]) {
  const tree = node(
    'root',
    0,
    Array.from({ length: parentCount }, (_, parent) =>
      node(
        `p${parent}`,
        1,
        Array.from({ length: childCount }, (_, child) =>
          node(`p${parent}/c${child}`, 2, [], 100 + ((parent * 13 + child * 29) % 150)),
        ),
      ),
    ),
  )
  const circles = buildBubbleCircles(tree, 1000, 760).filter((circle) => circle.id !== 'root')
  const world = new SoftBubbleWorld(circles, 6)
  const target = circles.find((circle) => circle.id === 'p0')
  if (!target) throw new Error('Benchmark drag target was culled')
  for (let step = 0; step < 30; step += 1) {
    world.drag(target.id, { x: target.x + 6 + 150, y: target.y + 6 + Math.sin(step / 20) * 40 })
    world.step()
  }
  const start = performance.now()
  for (let step = 0; step < 120; step += 1) {
    world.drag(target.id, { x: target.x + 6 + 150 * Math.cos(step / 30), y: target.y + 6 + 80 * Math.sin(step / 30) })
    world.step()
  }
  const duration = performance.now() - start
  const snapshot = world.snapshot()
  const membranes = snapshot.filter((body) => body.kind === 'membrane').length
  console.log(
    `${parentCount} × ${childCount}: ${(duration / 120).toFixed(3)} ms/step; ${membranes} membranes, ${snapshot.length - membranes} passengers`,
  )
}
