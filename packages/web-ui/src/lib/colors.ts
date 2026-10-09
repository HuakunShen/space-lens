import type { HierarchyNode } from 'd3-hierarchy'
import type { TreeSliceNode } from '../types'
import { CONCEPT_PALETTE, namedConceptFamily } from './concept-palette.ts'

/**
 * Color families sampled from the concept images. One branch of the tree draws from a single
 * family, so a folder and its contents read as a set of tints rather than a
 * scatter of unrelated colors — the difference between "these belong
 * together" and "these are nine arbitrary pastels".
 */
const COLOR_FAMILIES = CONCEPT_PALETTE.filter((family) => family.name !== 'gray')

/** All chart families share the same folder identity color. */
export function sunburstColor(id: string, depth: number, familySeed: string = id, familyName?: string): string {
  return nodeColor(id, depth, familySeed, familyName)
}

/**
 * The color for one node.
 *
 * `familySeed` is the id of the branch's top-level ancestor; it owns the
 * color. Known disk categories use the artwork's assignment; other branches
 * use a stable ID hash. Depth never changes the hue when switching charts.
 */
export function nodeColor(id: string, _depth: number, familySeed: string = id, familyName?: string): string {
  const family =
    namedConceptFamily(familyName ?? '') ??
    COLOR_FAMILIES[Math.abs(hash(familySeed)) % COLOR_FAMILIES.length] ??
    COLOR_FAMILIES[0]
  return family.treemap[0]
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
 * on a node's color within the same focused folder.
 */
export function branchColor(node: HierarchyNode<TreeSliceNode>): string {
  if (node.data.ignored) return nodeMutedColor(node.data.depth)
  // `ancestors()` runs self-first, root-last, so the depth-1 node is second
  // from the end. A depth-1 node is its own family, and the root has no
  // family to borrow, so both fall back to the node's own id.
  const family = node.ancestors().at(-2)?.data ?? node.data
  return nodeColor(node.data.id, node.data.depth, family.id, family.name)
}

function hash(input: string): number {
  let value = 0
  for (let index = 0; index < input.length; index += 1) {
    value = (value << 5) - value + input.charCodeAt(index)
    value |= 0
  }
  return value
}
