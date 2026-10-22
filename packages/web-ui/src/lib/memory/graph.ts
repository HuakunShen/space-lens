/**
 * The focus graph of the Memory view: "where does this process sit?"
 *
 * The tree view answers the question as a list; this module turns the same
 * `ProcessForest` into a node-link diagram around one process — the ancestor
 * chain above it, its siblings for context, and its own subtree below. The
 * geometry is computed here, in plain numbers, so the drawing library only has
 * to paint what it is given and the layout can be tested without a browser.
 *
 * The shapes are deliberately independent of Svelte Flow (`GraphNode` /
 * `GraphEdge` are our own): the component adapts them, and swapping the
 * renderer later never touches the tree logic.
 */
import {
  ancestryOf,
  type ProcessForest,
  type TreeNode,
  type TreeKind,
  type TreeProtection,
} from './tree.ts'

/** Node width in layout units; the rendered card uses the same width. */
export const GRAPH_NODE_WIDTH = 168
/** Horizontal distance between two leaf slots. */
export const GRAPH_X_GAP = 196
/** Vertical distance between two generations. */
export const GRAPH_Y_GAP = 96
/** How many nodes one diagram may hold before it starts folding branches. */
export const GRAPH_MAX_NODES = 80
/** How many siblings of the focused process are drawn for context. */
export const GRAPH_MAX_SIBLINGS = 4
/** How many children of one process are drawn before the rest fold into `+N`. */
export const GRAPH_MAX_CHILDREN = 6
/** How many nodes one generation may hold; keeps a 400-child fan-out readable. */
export const GRAPH_MAX_PER_LEVEL = 12
/** How far up the ancestor chain the diagram goes; the near end is kept. */
export const GRAPH_MAX_ANCESTORS = 30

export type GraphRole = 'ancestor' | 'focus' | 'sibling' | 'descendant'

export interface GraphNode {
  readonly pid: number
  readonly name: string
  readonly kind: TreeKind
  readonly groupName: string | null
  readonly role: GraphRole
  readonly cpuPercent: number | null
  readonly restricted: boolean
  readonly protection: TreeProtection | null
  /** Memory of this process alone; `null` when the snapshot has no reading. */
  readonly ownBytes: number | null
  /** Memory of this process plus every descendant it has in the snapshot. */
  readonly subtreeBytes: number
  /** Descendants in the snapshot, drawn or not (excludes the node itself). */
  readonly descendantCount: number
  /** Children of this node that the node cap left out of the diagram. */
  readonly hiddenChildren: number
  /**
   * This process's own memory as a fraction of the focused subtree (0..1).
   * The card uses it as a small scale bar.
   */
  readonly share: number
  /** Generation in the drawn tree; 0 is the topmost ancestor drawn. */
  readonly depth: number
  /** Center of the card, before zoom. The focused node sits at x = 0. */
  readonly x: number
  readonly y: number
}

export type GraphEdgeKind = 'chain' | 'branch' | 'context'

export interface GraphEdge {
  readonly source: number
  readonly target: number
  /** 'chain' is the ancestor path, 'branch' the focus's subtree. */
  readonly kind: GraphEdgeKind
}

export interface ProcessGraph {
  readonly focusPid: number
  readonly nodes: readonly GraphNode[]
  readonly edges: readonly GraphEdge[]
  /** The focus itself, for the panel header. */
  readonly focus: GraphNode
  /** Descendants of the focus in the snapshot, drawn or not. */
  readonly focusDescendantCount: number
  /** Ancestors cut off above the drawn root (kept the end of the chain). */
  readonly hiddenAncestors: number
  /** Some node in the diagram has children left out by the node cap. */
  readonly truncated: boolean
  /** Descendants of the focus that are not drawn. */
  readonly hiddenProcesses: number
}

interface Draft {
  readonly node: TreeNode
  readonly role: GraphRole
  children: Draft[]
  hiddenChildren: number
  depth: number
  x: number
  y: number
}

export interface GraphOptions {
  /** Upper bound on drawn nodes. The ancestor chain is never cut by it. */
  readonly maxNodes?: number
  /** Siblings of the focus drawn for context; 0 hides them. */
  readonly maxSiblings?: number
  /** Ancestors drawn above the focus; the end of a long chain is kept. */
  readonly maxAncestors?: number
  /** Children of one process drawn before the rest fold into `+N`. */
  readonly maxChildren?: number
  /** Nodes one generation may hold; a huge fan-out would be a wall of cards. */
  readonly maxPerLevel?: number
}

/**
 * Draw the neighbourhood of one process.
 *
 * The ancestor chain is always complete up to `maxAncestors` (a deep chain
 * keeps its *end*, so the focus is never cut off). Below the focus the subtree
 * is added one generation at a time — the biggest branches first, because the
 * forest sorts children by memory — capped per parent (`maxChildren`), per
 * generation (`maxPerLevel`) and in total (`maxNodes`); a process that
 * launched four hundred helpers must stay a tree rather than a single wall of
 * cards. A node whose children did not fit reports how many it hides, and the
 * card draws a `+N` badge.
 * Siblings are added as leaves so the branch point is visible instead of the
 * focus hanging off a one-child parent.
 *
 * Returns `null` for a pid that is not in the forest.
 */
export function buildProcessGraph(
  forest: ProcessForest,
  pid: number,
  options: GraphOptions = {},
): ProcessGraph | null {
  const focus = forest.byPid.get(pid)
  if (focus === undefined) return null

  const maxNodes = Math.max(2, Math.floor(options.maxNodes ?? GRAPH_MAX_NODES))
  const maxSiblings = Math.max(0, Math.floor(options.maxSiblings ?? GRAPH_MAX_SIBLINGS))
  const maxAncestors = Math.max(1, Math.floor(options.maxAncestors ?? GRAPH_MAX_ANCESTORS))
  const maxChildren = Math.max(1, Math.floor(options.maxChildren ?? GRAPH_MAX_CHILDREN))
  const maxPerLevel = Math.max(1, Math.floor(options.maxPerLevel ?? GRAPH_MAX_PER_LEVEL))

  const fullChain = ancestryOf(forest, pid)
  const hiddenAncestors = Math.max(0, fullChain.length - maxAncestors)
  const chain = hiddenAncestors === 0 ? fullChain : fullChain.slice(fullChain.length - maxAncestors)

  // The chain is built first and counted against the budget first, so no
  // amount of descendants can push an ancestor out of the picture.
  const chainDrafts: Draft[] = []
  for (let index = 0; index < chain.length; index += 1) {
    const draft = draftOf(chain[index]!, index === chain.length - 1 ? 'focus' : 'ancestor')
    if (index > 0) chainDrafts[index - 1]!.children.push(draft)
    chainDrafts.push(draft)
  }
  const focusDraft = chainDrafts[chainDrafts.length - 1]!
  let budget = Math.max(0, maxNodes - chainDrafts.length)

  // Siblings: the parent's other children, biggest first. They stay leaves
  // (their subtrees would be a diagram of their own) and the parent keeps the
  // natural order of its children, so the focus sits where it really sits.
  const parent = focus.ppid === null ? undefined : forest.byPid.get(focus.ppid)
  const parentDraft = chainDrafts.length >= 2 ? chainDrafts[chainDrafts.length - 2]! : undefined
  if (parent !== undefined && parentDraft !== undefined && parentDraft.node.pid === parent.pid) {
    const others = parent.children.filter((child) => child.pid !== focus.pid)
    const taken = new Set(others.slice(0, Math.min(others.length, maxSiblings, budget)).map((node) => node.pid))
    if (taken.size > 0) {
      const ordered: Draft[] = []
      for (const child of parent.children) {
        if (child.pid === focus.pid) ordered.push(focusDraft)
        else if (taken.has(child.pid)) {
          const draft = draftOf(child, 'sibling')
          draft.hiddenChildren = child.children.length
          ordered.push(draft)
        }
      }
      if (!ordered.includes(focusDraft)) ordered.push(focusDraft)
      parentDraft.children = ordered
      budget -= taken.size
    }
    // The siblings left out are hidden children of the parent, just like a
    // capped branch below the focus.
    if (others.length > taken.size) parentDraft.hiddenChildren = others.length - taken.size
  }

  // Descendants, one generation at a time. Each process contributes at most
  // `maxChildren` children and each generation at most `maxPerLevel` nodes:
  // a process that launched four hundred helpers must stay a tree, not become
  // a wall of cards. The forest sorts children by memory, so the branches kept
  // are the expensive ones; everything folded away is reported as `+N`.
  let frontier: Draft[] = [focusDraft]
  let drawnDescendants = 0
  while (frontier.length > 0) {
    const next: Draft[] = []
    for (const current of frontier) {
      const children = current.node.children
      const take = Math.max(
        0,
        Math.min(children.length, maxChildren, budget, maxPerLevel - next.length),
      )
      for (let index = 0; index < take; index += 1) {
        const child = draftOf(children[index]!, 'descendant')
        current.children.push(child)
        next.push(child)
        drawnDescendants += 1
        budget -= 1
      }
      if (take < children.length) current.hiddenChildren = children.length - take
    }
    frontier = next
  }

  // Tidy layout: leaves get consecutive slots, a parent sits centred over its
  // children. Post-order, so a parent is placed after the subtree it spans.
  let slots = 0
  const place = (draft: Draft, depth: number): void => {
    draft.depth = depth
    if (draft.children.length === 0) {
      draft.x = slots * GRAPH_X_GAP
      slots += 1
    } else {
      for (const child of draft.children) place(child, depth + 1)
      const first = draft.children[0]!
      const last = draft.children[draft.children.length - 1]!
      draft.x = (first.x + last.x) / 2
    }
    draft.y = depth * GRAPH_Y_GAP
  }
  place(chainDrafts[0]!, 0)

  // The ancestry is drawn as a straight spine. A tidy layout centres a parent
  // over *all* its children, so one sibling would push the whole chain
  // sideways and out of the pane; the chain is what the panel is for, so it
  // stays at the focus's x and the siblings branch off it instead.
  for (const draft of chainDrafts) draft.x = focusDraft.x

  // Put the focused card at x = 0, so re-rooting the diagram does not jump.
  const shift = focusDraft.x
  const focusSubtreeBytes = Math.max(1, focus.subtreeBytes)
  const nodes: GraphNode[] = []
  const edges: GraphEdge[] = []

  const emit = (draft: Draft): void => {
    const footprint = draft.node.footprintBytes ?? draft.node.residentBytes
    const ownBytes = footprint !== null && footprint > 0 ? footprint : null
    nodes.push({
      pid: draft.node.pid,
      name: draft.node.name,
      kind: draft.node.kind,
      groupName: draft.node.groupName,
      role: draft.role,
      cpuPercent: draft.node.cpuPercent,
      restricted: draft.node.restricted,
      protection: draft.node.protection,
      ownBytes,
      subtreeBytes: draft.node.subtreeBytes,
      descendantCount: draft.node.descendantCount,
      hiddenChildren: draft.hiddenChildren,
      share: Math.min(1, (ownBytes ?? 0) / focusSubtreeBytes),
      depth: draft.depth,
      x: draft.x - shift,
      y: draft.y,
    })
    for (const child of draft.children) {
      const kind: GraphEdgeKind =
        child.role === 'sibling' ? 'context' : child.role === 'descendant' ? 'branch' : 'chain'
      edges.push({ source: draft.node.pid, target: child.node.pid, kind })
      emit(child)
    }
  }
  emit(chainDrafts[0]!)

  const focusNode = nodes.find((node) => node.pid === pid)!
  return {
    focusPid: pid,
    nodes,
    edges,
    focus: focusNode,
    focusDescendantCount: focus.descendantCount,
    hiddenAncestors,
    truncated: nodes.some((node) => node.hiddenChildren > 0),
    hiddenProcesses: Math.max(0, focus.descendantCount - drawnDescendants),
  }
}

function draftOf(node: TreeNode, role: GraphRole): Draft {
  return { node, role, children: [], hiddenChildren: 0, depth: 0, x: 0, y: 0 }
}
