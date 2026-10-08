import assert from 'node:assert/strict'
import { test } from 'node:test'
import { buildTreemapTiles, clipTileName } from '../src/lib/treemap.ts'
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

const WIDTH = 620
const HEIGHT = 430

test('flat view shows only the first layer and tiles fill the canvas', () => {
  const root = node('root', 100, [node('big', 60, [node('inner', 10)]), node('small', 20)], { depth: 0 })
  const tiles = buildTreemapTiles(root, WIDTH, HEIGHT, false)
  assert.ok(tiles.every((tile) => tile.depth === 1), 'no nested layers in the flat view')
  assert.deepEqual(
    tiles.map((tile) => tile.id).sort(),
    ['big', 'small'],
    'deep truncation leaves the layer view untouched',
  )
  const area = tiles.reduce((sum, tile) => sum + (tile.x1 - tile.x0) * (tile.y1 - tile.y0), 0)
  assert.ok(Math.abs(area - WIDTH * HEIGHT) < WIDTH * HEIGHT * 0.02, `tiles cover the canvas (got ${area})`)
})

test('tile areas and shares track sibling sizes', () => {
  const root = node('root', 100, [node('big', 60), node('small', 20)], { depth: 0 })
  const tiles = buildTreemapTiles(root, WIDTH, HEIGHT, false)
  const big = tiles.find((tile) => tile.id === 'big')!
  const small = tiles.find((tile) => tile.id === 'small')!
  assert.ok((big.x1 - big.x0) * (big.y1 - big.y0) / ((small.x1 - small.x0) * (small.y1 - small.y0)) - 3 < 0.05)
  assert.ok(Math.abs(big.share - 0.75) < 1e-6)
  assert.ok(Math.abs(small.share - 0.25) < 1e-6)
})

test('nested view insets children below the parent name strip', () => {
  const root = node('root', 100, [node('big', 60, [node('inner', 10)])], { depth: 0 })
  const tiles = buildTreemapTiles(root, WIDTH, HEIGHT, true)
  const big = tiles.find((tile) => tile.id === 'big')!
  const inner = tiles.find((tile) => tile.id === 'inner')!
  assert.ok(inner, 'the nested view paints the second layer')
  assert.ok(inner.y0 >= big.y0 + 15, 'children start below the name strip')
  assert.ok(inner.x0 >= big.x0 && inner.x1 <= big.x1 && inner.y1 <= big.y1, 'children stay inside the parent')
  assert.equal(big.label, 'strip', 'a parent with painted children is a frame')
  assert.ok(inner.label === 'card' || inner.label === 'name')
})

test('omitted buckets surface as Other tiles exactly once', () => {
  const folder = node('folder', 80, [], { hasChildren: true, childCount: 8, omittedBytes: 80, omittedCount: 8 })
  const root = node('root', 100, [folder, node('file', 20)], { depth: 0, omittedBytes: 80, omittedCount: 8 })
  // Nested draws folder's children, so its Other bucket gets a tile; the
  // root's own omissions were already consumed by folder's and stay gone.
  const nested = buildTreemapTiles(root, WIDTH, HEIGHT, true)
  assert.deepEqual(
    nested.filter((tile) => tile.isAggregate).map((tile) => tile.id),
    ['folder:omitted'],
  )
  // Flat shows one layer only — the bucket folds into folder's own area.
  const flat = buildTreemapTiles(root, WIDTH, HEIGHT, false)
  assert.ok(flat.every((tile) => !tile.isAggregate))
})

test('colors agree with the sunburst palette and mute ignored entries', () => {
  const ignored = node('ignored', 10, [], { ignored: true })
  const plain = node('plain', 30)
  const root = node('root', 40, [plain, ignored], { depth: 0 })
  const tiles = buildTreemapTiles(root, WIDTH, HEIGHT, false)
  assert.equal(tiles.find((tile) => tile.id === 'plain')!.color, nodeColor('plain', 1))
  assert.equal(tiles.find((tile) => tile.id === 'ignored')!.color, nodeMutedColor(1))
})

test('labels scale with tile room and clip long names', () => {
  const root = node('root', 100, [node('wide', 99), node('sliver', 1)], { depth: 0 })
  const tiles = buildTreemapTiles(root, WIDTH, HEIGHT, false)
  const wide = tiles.find((tile) => tile.id === 'wide')!
  assert.equal(wide.label, 'card')
  assert.equal(clipTileName('node_modules', 100).length <= 'node_modules'.length, true)
  assert.equal(clipTileName('src', 200), 'src')
  assert.equal(clipTileName('anything', 10), '')
})
