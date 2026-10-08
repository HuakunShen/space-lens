import { hierarchy, treemap, treemapSquarify } from 'd3-hierarchy'
import { nodeColor, nodeMutedColor } from './colors.ts'
import { withOmittedBuckets } from './sunburst.ts'
import type { TreeSliceNode } from '../types'

export type ChartMode = 'sunburst' | 'flat' | 'nested'

/** How a tile's label renders — decided at build time from the tile's size. */
export type TreemapLabel = 'strip' | 'card' | 'name' | 'none'

export interface TreemapTile {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  /** Share of this tile among its own tiling siblings (0–1) — the flat
   * view's "who is biggest in this layer" percentage. */
  share: number
  color: string
  x0: number
  y0: number
  x1: number
  y1: number
  label: TreemapLabel
  isAggregate: boolean
  hasChildren: boolean
  node: TreeSliceNode
}

/** Gap between sibling tiles, in canvas pixels. */
const GAP = 2
/** Strip reserved above a nested parent's children for the parent's name. */
const STRIP = 15
/** Tiles skinnier than this on either axis are invisible noise. */
const MIN_SIDE = 2

/**
 * Squarified treemap tiles over the focused node's slice, in canvas pixels.
 *
 * Flat (the Burrow layer view): only the direct children of the focused
 * node tile the canvas, so siblings compare at a glance. Nested (the
 * webpack-bundle-analyzer form): roomy parents show their own children
 * inset, one inside the other — d3's `paddingTop` reserves each parent's
 * name strip, and the root gets none so the first layer still fills the
 * canvas.
 *
 * Children are normalized against each other exactly like the sunburst
 * (leaf sizes, parents contribute nothing), so both charts agree on
 * proportions and colors for the same slice.
 */
export function buildTreemapTiles(
  tree: TreeSliceNode,
  width: number,
  height: number,
  nested: boolean,
): TreemapTile[] {
  if (width <= 0 || height <= 0) return []
  const root = hierarchy(withOmittedBuckets(tree))
    .sum((node) => (node.children.length > 0 ? 0 : Math.max(1, node.size)))
    .sort((left, right) => (right.value ?? 0) - (left.value ?? 0))
  const laidOut = treemap<TreeSliceNode>()
    .tile(treemapSquarify)
    .size([width, height])
    .paddingInner(GAP)
    .paddingOuter(0)
    .paddingTop((node) => (nested && node.depth > 0 && (node.children?.length ?? 0) > 0 ? STRIP : 0))(root)

  return laidOut
    .descendants()
    .filter(
      (node) =>
        node.depth > 0 &&
        (nested || node.depth === 1) &&
        node.x1 - node.x0 > MIN_SIDE &&
        node.y1 - node.y0 > MIN_SIDE,
    )
    .map((node) => {
      const tileWidth = node.x1 - node.x0
      const tileHeight = node.y1 - node.y0
      // d3 leaves have `children === undefined`, not an empty array.
      const hasPaintedChildren =
        nested && (node.children?.some((child) => child.x1 - child.x0 > MIN_SIDE && child.y1 - child.y0 > MIN_SIDE) ?? false)
      let label: TreemapLabel = 'none'
      if (hasPaintedChildren) {
        if (tileWidth >= 56 && tileHeight >= STRIP + 12) label = 'strip'
      } else if (tileWidth >= 88 && tileHeight >= 54) {
        label = 'card'
      } else if (tileWidth >= 48 && tileHeight >= 20) {
        label = 'name'
      }
      return {
        id: node.data.id,
        name: node.data.name,
        path: node.data.path,
        size: node.data.size,
        depth: node.data.depth,
        childCount: node.data.childCount,
        share: node.parent && node.parent.value ? (node.value ?? 0) / node.parent.value : 0,
        color: node.data.ignored ? nodeMutedColor(node.data.depth) : nodeColor(node.data.id, node.data.depth),
        x0: node.x0,
        y0: node.y0,
        x1: node.x1,
        y1: node.y1,
        label,
        isAggregate: node.data.id === `${node.parent?.data.id}:omitted`,
        hasChildren: node.data.hasChildren,
        node: node.data,
      }
    })
}

/** SVG text has no truncation; clip long names to what the tile fits. */
export function clipTileName(name: string, tileWidth: number): string {
  const maxChars = Math.floor((tileWidth - 10) / 6.2)
  if (maxChars <= 0) return ''
  return name.length <= maxChars ? name : `${name.slice(0, Math.max(1, maxChars - 1))}…`
}
