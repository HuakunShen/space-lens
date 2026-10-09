import { hierarchy, partition } from 'd3-hierarchy'
import { branchColor } from './colors.ts'
import { withOmittedBuckets } from './sunburst.ts'
import type { TreeSliceNode } from '../types'

export interface IcicleSegment {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  /** Share of this segment among its own siblings (0–1). */
  share: number
  color: string
  x0: number
  y0: number
  x1: number
  y1: number
  labelVisible: boolean
  isAggregate: boolean
  hasChildren: boolean
  node: TreeSliceNode
}

/** One depth level's column, for the header row above the chart. */
export interface IcicleColumn {
  depth: number
  x: number
  width: number
}

/** Levels drawn left to right before the chart stops being readable. */
export const ICICLE_MAX_DEPTH = 4
/** A segment needs this much height before its name fits on one line. */
const MIN_LABEL_HEIGHT = 15
/** …and this much column width before that line has room to be a word. */
const MIN_LABEL_WIDTH = 58

/**
 * Partition-chart geometry: one column per depth level, and within a
 * column each node occupies a vertical span proportional to its share of
 * its parent.
 *
 * The reading order is the point — left to right is the path from the
 * focused folder downwards, so a file's full ancestry is one horizontal
 * scan instead of the sunburst's radial hunt. Columns are equal width
 * because depth is categorical, not quantitative; spending width on depth
 * would make the first two levels unreadably thin on a deep tree.
 *
 * Sizes are normalized against leaves only, exactly like the sunburst and
 * the treemap, so all five charts agree on proportions and colors for the
 * same slice.
 */
export function buildIcicleSegments(
  tree: TreeSliceNode,
  width: number,
  height: number,
  maxDepth: number = ICICLE_MAX_DEPTH,
): IcicleSegment[] {
  if (width <= 0 || height <= 0 || maxDepth < 1) return []
  const root = hierarchy(withOmittedBuckets(tree))
    .sum((node) => (node.children.length > 0 ? 0 : Math.max(1, node.size)))
    .sort((left, right) => (right.value ?? 0) - (left.value ?? 0))

  // `partition` spends its second axis on depth, but that axis is scaled by
  // the hierarchy's own height rather than by `maxDepth` — so its y bands
  // are only equal when the tree happens to be exactly `maxDepth` deep. The
  // column is therefore derived from `node.depth` directly, which keeps
  // columns equal at any tree depth; only the first axis, the vertical
  // span, is read back off the layout.
  const laidOut = partition<TreeSliceNode>().size([height, 1])(root)
  const columnWidth = width / maxDepth

  return laidOut
    .descendants()
    .filter((node) => node.depth > 0 && node.depth <= maxDepth)
    .map((node) => {
      const span = node.x1 - node.x0
      const share = node.parent && node.parent.value ? (node.value ?? 0) / node.parent.value : 0
      return {
        id: node.data.id,
        name: node.data.name,
        path: node.data.path,
        size: node.data.size,
        depth: node.data.depth,
        childCount: node.data.childCount,
        share,
        color: branchColor(node),
        x0: (node.depth - 1) * columnWidth,
        y0: node.x0,
        x1: node.depth * columnWidth,
        y1: node.x1,
        labelVisible: columnWidth >= MIN_LABEL_WIDTH && span >= MIN_LABEL_HEIGHT,
        isAggregate: node.data.id === `${node.parent?.data.id}:omitted`,
        hasChildren: node.data.hasChildren,
        node: node.data,
      }
    })
}

/**
 * The columns a segment list actually fills, for the header strip. Derived
 * from the segments rather than from `maxDepth`, so a shallow folder does
 * not render three empty "Level 3" headers.
 */
export function icicleColumns(
  segments: IcicleSegment[],
  width: number,
  maxDepth: number = ICICLE_MAX_DEPTH,
): IcicleColumn[] {
  const deepest = segments.reduce((depth, segment) => Math.max(depth, segment.depth), 0)
  if (deepest === 0 || width <= 0) return []
  const levels = Math.min(deepest, maxDepth)
  const columnWidth = width / maxDepth
  return Array.from({ length: levels }, (_, index) => ({
    depth: index + 1,
    x: index * columnWidth,
    width: columnWidth,
  }))
}

/** SVG text has no truncation; clip long names to what the column fits. */
export function clipIcicleName(name: string, segmentWidth: number): string {
  const maxChars = Math.floor((segmentWidth - 12) / 6.2)
  if (maxChars <= 0) return ''
  return name.length <= maxChars ? name : `${name.slice(0, Math.max(1, maxChars - 1))}…`
}
