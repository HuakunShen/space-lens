import type { HierarchyNode } from 'd3-hierarchy'
import type { TreeSliceNode } from '../types'

/**
 * Hue families, in degrees. One branch of the tree draws from a single
 * family, so a folder and its contents read as a set of tints rather than a
 * scatter of unrelated colors — the difference between "these belong
 * together" and "these are nine arbitrary pastels".
 */
const HUE_FAMILIES = [212, 268, 148, 32, 336, 186, 46, 240, 88, 6]

/**
 * Lightness and saturation per level. Depth then reads off the chart
 * without a legend: children are a step lighter or deeper than their
 * parent, and the cycle is short enough that no level collides with the
 * one directly above it.
 */
const LEVEL_TINTS = [
  { lightness: 54, saturation: 80 },
  { lightness: 62, saturation: 76 },
  { lightness: 48, saturation: 82 },
  { lightness: 66, saturation: 70 },
]

/**
 * The color for one node.
 *
 * `familySeed` is the id of the branch's top-level ancestor; it owns the
 * hue. The node's own id nudges the hue a few degrees inside the family so
 * siblings stay apart, and `depth` picks the tint. It defaults to coloring
 * by the node's own id, which is what a caller with no ancestry to offer (a
 * flat list row, a legend swatch) wants.
 */
export function nodeColor(id: string, depth: number, familySeed: string = id): string {
  const level = Math.max(1, depth)
  const family = HUE_FAMILIES[Math.abs(hash(familySeed)) % HUE_FAMILIES.length] ?? HUE_FAMILIES[0]
  const offset = (Math.abs(hash(id)) % 23) - 11
  const hue = (((family + offset + (level - 1) * 5) % 360) + 360) % 360
  const tint = LEVEL_TINTS[(level - 1) % LEVEL_TINTS.length] ?? LEVEL_TINTS[0]
  return `hsl(${hue}, ${tint.saturation}%, ${tint.lightness}%)`
}

/**
 * The flat grey for entries the scan did not count. Deliberately not a tint
 * of any family: "not measured" should never look like "measured, and
 * small".
 */
export function nodeMutedColor(depth: number): string {
  return depth % 2 === 0 ? '#3a3f49' : '#484d57'
}

/**
 * The color for one laid-out hierarchy node. Every chart builder funnels
 * through here, so the sunburst, treemap, icicle, bubbles, and strips agree
 * on a node's color and drilling into a folder never re-hues it.
 */
export function branchColor(node: HierarchyNode<TreeSliceNode>): string {
  if (node.data.ignored) return nodeMutedColor(node.data.depth)
  // `ancestors()` runs self-first, root-last, so the depth-1 node is second
  // from the end. A depth-1 node is its own family, and the root has no
  // family to borrow, so both fall back to the node's own id.
  const family = node.ancestors().at(-2)?.data.id ?? node.data.id
  return nodeColor(node.data.id, node.data.depth, family)
}

function hash(input: string): number {
  let value = 0
  for (let index = 0; index < input.length; index += 1) {
    value = (value << 5) - value + input.charCodeAt(index)
    value |= 0
  }
  return value
}
