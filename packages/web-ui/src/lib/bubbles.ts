import { hierarchy, pack } from 'd3-hierarchy'
import { branchColor } from './colors.ts'
import { withOmittedBuckets } from './sunburst.ts'
import type { TreeSliceNode } from '../types'

export interface BubbleCircle {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  /** Share of this circle among its own siblings (0–1). */
  share: number
  color: string
  x: number
  y: number
  r: number
  /** Room enough for a name and a size line inside the circle. */
  labelVisible: boolean
  isAggregate: boolean
  hasChildren: boolean
  node: TreeSliceNode
}

/** Levels painted besides the faint root backdrop: children, grandchildren. */
export const BUBBLE_MAX_DEPTH = 2
/** Radius below which a label would spill outside its own circle. */
const MIN_LABEL_RADIUS = 21
/**
 * Radius below which a circle is not worth painting. A real `node_modules`
 * packs hundreds of grandchildren; without a floor the canvas fills with
 * sub-pixel dots that cost render time and hide the shapes that matter.
 */
const MIN_RADIUS = 6
/** Breathing room between touching circles, in canvas pixels. */
const BUBBLE_PADDING = 5

/**
 * Packed-circle geometry: sibling circles nested inside their parent, with
 * **area** proportional to size.
 *
 * Area — not radius — is the honest mapping: a folder twice as big gets
 * twice the ink. It is also why bubbles are a worse analysis tool than the
 * treemap for the same data: comparing areas of circles by eye is
 * noticeably harder than comparing rectangles that share edges, and a
 * packed layout leaves the ragged gaps the treemap spends on tiles. The
 * win is shape recognition and a calmer overview, so this view is for
 * orientation, not for ranking.
 *
 * Depth 0 is kept so the caller can paint it as one faint backdrop, the way
 * a pack reads as a single body rather than a scattered field.
 */
export function buildBubbleCircles(tree: TreeSliceNode, width: number, height: number): BubbleCircle[] {
  if (width <= 0 || height <= 0) return []
  const root = hierarchy(withOmittedBuckets(tree))
    .sum((node) => (node.children.length > 0 ? 0 : Math.max(1, node.size)))
    .sort((left, right) => (right.value ?? 0) - (left.value ?? 0))

  const packed = pack<TreeSliceNode>().size([width, height]).padding(BUBBLE_PADDING)(root)
  // Inset the child pack as a whole. Its circles keep their relative areas
  // and separation while the parent gains a clear header for its identity.
  for (const parent of packed.children ?? []) {
    if (!parent.children) continue
    for (const child of parent.descendants().slice(1)) {
      child.x = parent.x + (child.x - parent.x) * 0.8
      child.y = parent.y + (child.y - parent.y) * 0.8 + parent.r * 0.14
      child.r *= 0.8
    }
  }
  // The depth-0 circle is only a backdrop for the circles nested inside it.
  // An empty or unmeasured folder packs to a single full-canvas circle, which
  // would read as "this folder is the whole disk" — so it is dropped instead.
  // The decision is structural (hierarchy depth), not `data.depth`, which is
  // the caller's absolute depth and need not start at zero.
  const hasNested = (packed.children?.length ?? 0) > 0

  return packed
    .descendants()
    .filter((node) => node.depth <= BUBBLE_MAX_DEPTH && node.r >= MIN_RADIUS && (node.depth > 0 || hasNested))
    .map((node) => ({
      id: node.data.id,
      name: node.data.name,
      path: node.data.path,
      size: node.data.size,
      depth: node.data.depth,
      childCount: node.data.childCount,
      share: node.parent && node.parent.value ? (node.value ?? 0) / node.parent.value : 0,
      color: branchColor(node),
      x: node.x,
      y: node.y,
      r: node.r,
      labelVisible: node.r >= MIN_LABEL_RADIUS,
      isAggregate: node.data.id === `${node.parent?.data.id}:omitted`,
      hasChildren: node.data.hasChildren,
      node: node.data,
    }))
}

/** SVG text has no truncation; clip names to the widest chord that fits. */
export function clipBubbleName(name: string, radius: number): string {
  const maxChars = Math.floor((radius * 1.7) / 6.2)
  // Below three characters the ellipsis is longer than the name it hides;
  // the caller's `labelVisible` already suppresses most of these.
  if (maxChars < 3) return ''
  return name.length <= maxChars ? name : `${name.slice(0, maxChars - 1)}…`
}
