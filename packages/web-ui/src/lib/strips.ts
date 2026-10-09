import { nodeColor, nodeMutedColor } from './colors.ts'
import { withOmittedBuckets } from './sunburst.ts'
import type { TreeSliceNode } from '../types'

/** One child's slice of its parent row. */
export interface StripSegment {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  /** Share of the row this segment belongs to (0–1) — also its painted width. */
  share: number
  color: string
  /** True for the "Other" bucket that stands in for omitted entries. */
  isAggregate: boolean
  ignored: boolean
  hasChildren: boolean
  node: TreeSliceNode
}

/** One ranked folder row: a label, a size, and a proportional bar. */
export interface StripRow {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  hasChildren: boolean
  ignored: boolean
  collapsed: boolean
  /** Share of the focused folder (0–1), normalized like every other chart. */
  share: number
  color: string
  /** True for the "Other" bucket that stands in for omitted entries. */
  isAggregate: boolean
  segments: StripSegment[]
  node: TreeSliceNode
}

/** One entry of the "largest items" rail beside the rows. */
export interface RankedItem {
  id: string
  name: string
  path: string
  size: number
  depth: number
  /** Share of the focused folder's measured size (0–1). */
  share: number
  color: string
  hasChildren: boolean
  ignored: boolean
  node: TreeSliceNode
}

/**
 * Ranked proportional strips: one row per direct child of the focused
 * folder, biggest first, each row split into the proportional slices of
 * *its* children.
 *
 * The rows answer "which folders matter" in a single downward scan of a
 * left-aligned rank column, and the slices answer "what is inside the
 * interesting one" without leaving the view — the two questions a disk
 * cleanup actually starts from. Percentages are printed rather than left
 * to the eye, because this is the view meant for reading numbers.
 *
 * Rows are weighted by leaf size, exactly like the sunburst, treemap,
 * icicle, and bubbles, so a percentage means the same thing in every
 * chart. `size` stays the measured subtree size, which is the number a
 * person compares against their disk.
 */
export function buildStripRows(tree: TreeSliceNode): StripRow[] {
  const root = withOmittedBuckets(tree)
  const children = [...root.children].sort(byLeafValueDesc)
  const total = children.reduce((sum, child) => sum + leafValue(child), 0)
  if (total <= 0) return []

  return children.map((child) => {
    const weight = leafValue(child)
    const grandchildren = [...child.children].sort(byLeafValueDesc)
    const rowTotal = grandchildren.reduce((sum, node) => sum + leafValue(node), 0)
    return {
      id: child.id,
      name: child.name,
      path: child.path,
      size: child.size,
      depth: child.depth,
      childCount: child.childCount,
      hasChildren: child.hasChildren,
      ignored: child.ignored,
      collapsed: child.collapsed,
      share: weight / total,
      color: rowColor(child),
      isAggregate: child.id === `${root.id}:omitted`,
      segments:
        rowTotal > 0
          ? grandchildren.map((node) => ({
              id: node.id,
              name: node.name,
              path: node.path,
              size: node.size,
              depth: node.depth,
              childCount: node.childCount,
              share: leafValue(node) / rowTotal,
              color: segmentColor(node, child.id),
              isAggregate: node.id === `${child.id}:omitted`,
              ignored: node.ignored,
              hasChildren: node.hasChildren,
              node,
            }))
          : [],
      node: child,
    }
  })
}

/**
 * The biggest measured entries in the slice, for the rail beside the rows.
 *
 * Ranks one level below the rows — the folders *inside* the biggest folders —
 * because ranking the rows again would only repeat them, and because the
 * actionable entries on a full disk (`node_modules`, `DerivedData`,
 * `Caches`) live at that level. A slice with nothing deeper falls back to
 * the rows themselves rather than rendering an empty rail.
 */
export function rankLargestItems(tree: TreeSliceNode, limit: number): RankedItem[] {
  if (limit <= 0) return []
  const root = withOmittedBuckets(tree)
  const deeper = root.children.flatMap((child) => child.children)
  const pool = deeper.length > 0 ? deeper : root.children
  const total = Math.max(1, root.size)
  return [...pool]
    .sort(byMeasuredSizeDesc)
    .slice(0, limit)
    .map((node) => ({
      id: node.id,
      name: node.name,
      path: node.path,
      size: node.size,
      depth: node.depth,
      share: node.size / total,
      color: rowColor(node),
      hasChildren: node.hasChildren,
      ignored: node.ignored,
      node,
    }))
}

/**
 * The leaf-sum d3's `sum()` produces for the other four charts: a folder
 * contributes its children's total, never its own measured size, so an
 * ancestor is never counted twice against its own contents.
 */
function leafValue(node: TreeSliceNode): number {
  if (node.children.length === 0) return Math.max(1, node.size)
  return node.children.reduce((sum, child) => sum + leafValue(child), 0)
}

function byLeafValueDesc(left: TreeSliceNode, right: TreeSliceNode): number {
  return leafValue(right) - leafValue(left)
}

function byMeasuredSizeDesc(left: TreeSliceNode, right: TreeSliceNode): number {
  return right.size - left.size
}

/**
 * Strips have no laid-out hierarchy to hand `branchColor`, so the family is
 * resolved here instead: a row owns the family its hue comes from, and a
 * segment inherits the family its parent row established.
 */
function rowColor(node: TreeSliceNode): string {
  return node.ignored ? nodeMutedColor(node.depth) : nodeColor(node.id, node.depth, node.id)
}

function segmentColor(node: TreeSliceNode, family: string): string {
  return node.ignored ? nodeMutedColor(node.depth) : nodeColor(node.id, node.depth, family)
}
