import { hierarchy, partition } from 'd3-hierarchy'
import { arc } from 'd3-shape'
import type { HierarchyRectangularNode } from 'd3-hierarchy'
import { branchColor } from './colors.ts'
import type { TreeSliceNode } from '../types'

export interface SunburstSegment {
  id: string
  name: string
  path: string
  size: number
  depth: number
  childCount: number
  hasChildren: boolean
  color: string
  pathData: string
  labelX: number
  labelY: number
  labelRotation: number
  labelVisible: boolean
  isAggregate: boolean
  node: TreeSliceNode
}

/**
 * Ring geometry over the focused node's slice.
 *
 * `innerRadius` is the radius of the hole the first ring starts at, and the
 * rings then divide the space between it and `radius`.
 *
 * That offset is the whole point: d3's partition always scales its depth axis
 * from zero, so laying it out over `radius` alone puts the first ring at
 * `radius / (height + 1)` and leaves the entire inner half of the chart empty —
 * a dead well that grew with every extra level. Laying the axis out over
 * `radius - innerRadius` and adding the well back to each ring keeps the hole
 * the caller asked for and gives every ring its share of the rest.
 */
export function buildSunburstSegments(tree: TreeSliceNode, radius: number, innerRadius = 62): SunburstSegment[] {
  const root = hierarchy(withOmittedBuckets(tree))
    .sum((node) => (node.children.length > 0 ? 0 : Math.max(1, node.size)))
    .sort((left, right) => (right.value ?? 0) - (left.value ?? 0))

  const well = Math.max(0, Math.min(innerRadius, radius - 24))
  const laidOut = partition<TreeSliceNode>().size([Math.PI * 2, Math.max(1, radius - well)])(root)
  const makeArc = arc<HierarchyRectangularNode<TreeSliceNode>>()
    .startAngle((node) => node.x0)
    .endAngle((node) => node.x1)
    .innerRadius((node) => well + node.y0 + 3)
    .outerRadius((node) => well + Math.max(node.y0 + 6, node.y1 - 3))
    .cornerRadius(1)
    .padAngle(0.002)

  return laidOut
    .descendants()
    .filter((node) => node.depth > 0)
    .map((node) => {
      const middleAngle = (node.x0 + node.x1) / 2
      const middleRadius = (node.y0 + node.y1) / 2
      const degrees = (middleAngle * 180) / Math.PI - 90
      return {
        id: node.data.id,
        name: node.data.name,
        path: node.data.path,
        size: node.data.size,
        depth: node.data.depth,
        childCount: node.data.childCount,
        hasChildren: node.data.hasChildren,
        color: branchColor(node),
        pathData: makeArc(node) ?? '',
        labelX: Math.cos(middleAngle - Math.PI / 2) * middleRadius,
        labelY: Math.sin(middleAngle - Math.PI / 2) * middleRadius,
        labelRotation: degrees > 90 ? degrees + 180 : degrees,
        labelVisible: node.x1 - node.x0 > 0.16 && node.y1 - node.y0 > 24,
        isAggregate: node.data.id === `${node.parent?.data.id}:omitted`,
        node: node.data,
      }
    })
}

export function withOmittedBuckets(node: TreeSliceNode): TreeSliceNode {
  const children = node.children.map(withOmittedBuckets)
  // Both hosts report subtree-wide omission totals. Descendant omissions already
  // belong to their own rings; adding them here again inflates every ancestor.
  const omittedBytes = Math.max(
    0,
    node.omittedBytes - node.children.reduce((sum, child) => sum + child.omittedBytes, 0),
  )
  const omittedCount = Math.max(
    0,
    node.omittedCount - node.children.reduce((sum, child) => sum + child.omittedCount, 0),
  )
  if (omittedBytes > 0 || omittedCount > 0) {
    children.push({
      id: `${node.id}:omitted`,
      name: 'Other',
      path: node.path,
      size: omittedBytes,
      depth: node.depth + 1,
      ignored: false,
      collapsed: false,
      hasChildren: false,
      childCount: omittedCount,
      children: [],
      omittedBytes: 0,
      omittedCount: 0,
    })
  }
  return {
    ...node,
    children,
  }
}
