/**
 * Process-tree derivation for the Memory view: "which process started this
 * one?"
 *
 * The snapshot already carries the parent pid on every process row
 * (`ProcessRow.ppid`), so the tree is derived here — no transport change, no
 * Rust change. A process whose parent is not in the snapshot (it exited, or it
 * belongs to another user and is hidden) becomes a root and is flagged as an
 * orphan; a parent link that would close a cycle is dropped instead of
 * hanging the view.
 *
 * Pure functions only: they take a snapshot and return new data, never mutate
 * the snapshot. The tree is a *view* concern, so it lives next to
 * `lib/memory/table.ts` rather than in the Rust core — the safety rules stay
 * in `crates/memory`.
 */
import type { MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'

/** The contract ships `kind` and `protection` as literal unions, not named types. */
export type TreeKind = ProcessGroup['kind']
export type TreeProtection = NonNullable<ProcessGroup['protection']>

export interface TreeNode {
  readonly pid: number
  readonly ppid: number | null
  readonly startTimeUs: number
  readonly name: string
  readonly groupId: string
  /** The application group this process belongs to; `null` for a lone CLI process. */
  readonly groupName: string | null
  readonly kind: TreeKind
  readonly cpuPercent: number | null
  readonly footprintBytes: number | null
  readonly residentBytes: number | null
  readonly protection: TreeProtection | null
  readonly restricted: boolean
  /** 0 for a root, 1 for its children, and so on. */
  depth: number
  /** The parent pid is not in the snapshot (it exited or is hidden from us). */
  orphan: boolean
  /** How many processes hang below this one, excluding itself. */
  descendantCount: number
  /** The memory of this process and everything below it. */
  subtreeBytes: number
  children: TreeNode[]
}

export interface ProcessForest {
  readonly roots: TreeNode[]
  readonly byPid: ReadonlyMap<number, TreeNode>
  /** Processes whose parent is not in the snapshot. */
  readonly orphanCount: number
  /** Parent links dropped because they closed a cycle. */
  readonly cycleCount: number
}

export interface TreeRow {
  readonly node: TreeNode
  readonly depth: number
  readonly hasChildren: boolean
  readonly expanded: boolean
  /** The ancestors of this node, root first, excluding the node itself. */
  readonly ancestors: readonly TreeNode[]
}

/**
 * Build the process forest of one snapshot.
 *
 * Roots are the processes without a usable parent: no parent pid at all, a
 * parent that is not in the snapshot, or a parent that is the process itself.
 * Children are ordered by footprint (then resident memory, name and pid) so
 * the heavy processes sit on top.
 */
export function buildForest(snap: MemorySnapshot): ProcessForest {
  const groupNames = new Map<string, string>()
  for (const group of snap.groups) groupNames.set(group.groupId, group.name)

  const nodes = new Map<number, TreeNode>()
  for (const process of snap.processes) {
    nodes.set(process.pid, {
      pid: process.pid,
      ppid: process.ppid,
      startTimeUs: process.startTimeUs,
      name: process.name,
      groupId: process.groupId,
      groupName: groupNames.get(process.groupId) ?? null,
      kind: process.kind,
      cpuPercent: process.cpuPercent,
      footprintBytes: process.footprintBytes,
      residentBytes: process.residentBytes,
      protection: process.protection,
      restricted: process.restricted,
      depth: 0,
      orphan: false,
      descendantCount: 0,
      subtreeBytes: 0,
      children: [],
    })
  }

  // The parent of each process, or `undefined` when the parent is gone (it
  // exited, or it is hidden from this user) or the process is its own parent.
  const parentOf = new Map<number, TreeNode | undefined>()
  let orphanCount = 0
  for (const node of nodes.values()) {
    if (node.ppid === null) {
      parentOf.set(node.pid, undefined)
      continue
    }
    const parent = nodes.get(node.ppid)
    if (parent === undefined || parent.pid === node.pid) {
      parentOf.set(node.pid, undefined)
      orphanCount += 1
      node.orphan = true
      continue
    }
    parentOf.set(node.pid, parent)
  }

  // A process has exactly one parent, so the parent links form a functional
  // graph: every chain ends at a root or in a cycle. Break one link per cycle
  // before linking the children, so the forest is always renderable.
  const cycles = findCycles(nodes, parentOf)
  for (const cycle of cycles) {
    const member = cycle.reduce((lowest, node) => (node.pid < lowest.pid ? node : lowest))
    parentOf.set(member.pid, undefined)
  }

  const roots: TreeNode[] = []
  for (const node of nodes.values()) {
    const parent = parentOf.get(node.pid)
    if (parent === undefined) {
      roots.push(node)
      continue
    }
    parent.children.push(node)
  }

  const visited = new Set<number>()
  for (const root of roots) walk(root, visited, 0)
  for (const root of roots) aggregate(root)

  for (const root of roots) sortBranch(root)
  roots.sort(compareNodes)

  return { roots, byPid: nodes, orphanCount, cycleCount: cycles.length }
}

/**
 * Sum each branch from the leaves up: how many processes hang below a node and
 * how much memory they hold together. A collapsed branch shows these instead
 * of its hidden rows, the way htop marks a collapsed subtree.
 */
function aggregate(node: TreeNode): { count: number; bytes: number } {
  let count = 0
  let bytes = node.footprintBytes ?? node.residentBytes ?? 0
  for (const child of node.children) {
    const childTotals = aggregate(child)
    count += childTotals.count + 1
    bytes += childTotals.bytes
  }
  node.descendantCount = count
  node.subtreeBytes = bytes
  return { count, bytes }
}

/**
 * Find every cycle in the parent links. Each process has at most one parent,
 * so the cycles are disjoint and one broken link per cycle is enough.
 */
function findCycles(
  nodes: ReadonlyMap<number, TreeNode>,
  parentOf: ReadonlyMap<number, TreeNode | undefined>,
): TreeNode[][] {
  const UNKNOWN = 0
  const VISITING = 1
  const DONE = 2
  const state = new Map<number, number>()
  const cycles: TreeNode[][] = []

  for (const start of nodes.values()) {
    if (state.get(start.pid) === DONE) continue
    const chain: TreeNode[] = []
    const position = new Map<number, number>()
    let current: TreeNode | undefined = start
    while (current !== undefined) {
      const known = state.get(current.pid) ?? UNKNOWN
      if (known === DONE) break
      if (known === VISITING) {
        const at = position.get(current.pid)
        if (at !== undefined) cycles.push(chain.slice(at))
        break
      }
      state.set(current.pid, VISITING)
      position.set(current.pid, chain.length)
      chain.push(current)
      current = parentOf.get(current.pid)
    }
    for (const node of chain) state.set(node.pid, DONE)
  }
  return cycles
}

/** Depth-first walk that assigns each node its depth below the root. */
function walk(node: TreeNode, visited: Set<number>, depth: number): void {
  if (visited.has(node.pid)) return
  visited.add(node.pid)
  node.depth = depth
  for (const child of node.children) walk(child, visited, depth + 1)
}

/** Sort one branch by footprint, then resident memory, then name and pid. */
function sortBranch(node: TreeNode): void {
  node.children.sort(compareNodes)
  for (const child of node.children) sortBranch(child)
}

function compareNodes(a: TreeNode, b: TreeNode): number {
  const aBytes = a.footprintBytes ?? a.residentBytes ?? 0
  const bBytes = b.footprintBytes ?? b.residentBytes ?? 0
  if (aBytes !== bBytes) return bBytes - aBytes
  const byName = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
  if (byName !== 0) return byName
  return a.pid - b.pid
}

export interface TreeSearch {
  /** The processes whose name, group name or pid matches the needle. */
  readonly matchPids: ReadonlySet<number>
  /** The matches plus every ancestor needed to reach them. */
  readonly keepPids: ReadonlySet<number>
}

/**
 * Match processes by name, group name or pid and collect the ancestors that
 * lead to them, so the view can show "java ← node ← launchd" instead of a
 * flat list of `java` rows.
 */
export function searchForest(forest: ProcessForest, needle: string): TreeSearch {
  const trimmed = needle.trim().toLowerCase()
  const matchPids = new Set<number>()
  if (trimmed === '') return { matchPids, keepPids: new Set<number>() }

  for (const node of forest.byPid.values()) {
    const matches =
      node.name.toLowerCase().includes(trimmed) ||
      (node.groupName !== null && node.groupName.toLowerCase().includes(trimmed)) ||
      String(node.pid).includes(trimmed)
    if (matches) matchPids.add(node.pid)
  }
  return { matchPids, keepPids: withAncestors(forest, matchPids) }
}

/**
 * The given processes plus every ancestor that leads to them, so the view can
 * show "java ← node ← launchd" instead of a flat list of `java` rows.
 */
export function withAncestors(forest: ProcessForest, pids: ReadonlySet<number>): Set<number> {
  const keep = new Set<number>()
  for (const pid of pids) {
    let current = forest.byPid.get(pid)
    while (current !== undefined) {
      if (keep.has(current.pid)) break
      keep.add(current.pid)
      current = current.ppid === null ? undefined : forest.byPid.get(current.ppid)
    }
  }
  return keep
}

const EMPTY_PIDS: ReadonlySet<number> = new Set<number>()

/**
 * Flatten the forest into the rows the view renders.
 *
 * `keep` restricts the result to a subtree (the search case): a node is only
 * rendered when it is in `keep`, and the kept chain is always expanded. Without
 * `keep`, the whole forest is rendered and `collapsed` decides which branches
 * the user has closed; a branch nobody closed stays open.
 */
export function flattenForest(
  forest: ProcessForest,
  options: {
    /** Nodes known to be open. When omitted the tree is open unless `collapsed`. */
    expanded?: ReadonlySet<number>
    /** Nodes the user closed; only consulted when `expanded` is not given. */
    collapsed?: ReadonlySet<number>
    keep?: ReadonlySet<number> | null
    /** When `keep` is set, the kept chain is open unless this is false. */
    expandKept?: boolean
  } = {},
): TreeRow[] {
  const expanded = options.expanded
  const collapsed = options.collapsed ?? EMPTY_PIDS
  const keep = options.keep ?? null
  const expandKept = options.expandKept ?? true
  const rows: TreeRow[] = []
  const ancestors: TreeNode[] = []

  const isOpen = (pid: number): boolean => {
    if (keep !== null && expandKept) return true
    if (expanded !== undefined) return expanded.has(pid)
    // A tree that starts folded hides the branches it exists to show, so the
    // default is open and the user's collapses are what close it.
    return !collapsed.has(pid)
  }

  const visit = (node: TreeNode, depth: number): void => {
    if (keep !== null && !keep.has(node.pid)) return
    const children = keep === null ? node.children : node.children.filter((child) => keep.has(child.pid))
    const open = isOpen(node.pid)
    rows.push({ node, depth, hasChildren: children.length > 0, expanded: open, ancestors: [...ancestors] })
    if (!open) return
    ancestors.push(node)
    for (const child of children) visit(child, depth + 1)
    ancestors.pop()
  }

  for (const root of forest.roots) visit(root, 0)
  return rows
}

/**
 * The ancestry of one process, root first, ending with the process itself.
 * Empty when the pid is unknown to the forest.
 */
export function ancestryOf(forest: ProcessForest, pid: number): TreeNode[] {
  const chain: TreeNode[] = []
  let current = forest.byPid.get(pid)
  while (current !== undefined) {
    chain.push(current)
    current = current.ppid === null ? undefined : forest.byPid.get(current.ppid)
    if (chain.length > 256) break
  }
  return chain.reverse()
}

// ---------------------------------------------------------------------------
// The group tree
// ---------------------------------------------------------------------------

export interface GroupTree {
  /** The group that launched each group; `null` for a root or an unseen parent. */
  readonly parentOf: ReadonlyMap<string, string | null>
  /** The groups each group launched, largest footprint first. */
  readonly childrenOf: ReadonlyMap<string, string[]>
}

/**
 * The group-level view of the process tree: which app group launched which.
 *
 * The advisor navigates by group id, not by pid, so it needs the tree one
 * level up from `buildForest`. A group's parent is the group of the parent of
 * its main process — the process the group is named after — so a bundle's
 * helpers fold into their app and the chain reads `rustc ← cargo ← zsh ←
 * Claude`. A group whose parent is not in the snapshot is a root.
 */
export function buildGroupTree(snap: MemorySnapshot): GroupTree {
  const processByPid = new Map<number, ProcessRow>()
  for (const process of snap.processes) processByPid.set(process.pid, process)

  const footprintOf = new Map<string, number>()
  for (const group of snap.groups) footprintOf.set(group.groupId, group.footprintBytes)

  const parentOf = new Map<string, string | null>()
  for (const group of snap.groups) {
    const main = processByPid.get(group.mainPid)
    let parent: string | null = null
    if (main !== undefined && main.ppid !== null && main.ppid !== main.pid) {
      const parentProcess = processByPid.get(main.ppid)
      if (parentProcess !== undefined && parentProcess.groupId !== group.groupId) {
        parent = parentProcess.groupId
      }
    }
    parentOf.set(group.groupId, parent)
  }

  const childrenOf = new Map<string, string[]>()
  for (const group of snap.groups) {
    const parent = parentOf.get(group.groupId) ?? null
    if (parent === null) continue
    const siblings = childrenOf.get(parent) ?? []
    siblings.push(group.groupId)
    childrenOf.set(parent, siblings)
  }
  for (const [parent, siblings] of childrenOf) {
    siblings.sort((a, b) => (footprintOf.get(b) ?? 0) - (footprintOf.get(a) ?? 0))
    childrenOf.set(parent, siblings)
  }

  return { parentOf, childrenOf }
}

/**
 * The chain of groups that led to one group, root first, the group itself
 * last. Empty when the group id is unknown.
 */
export function groupAncestry(tree: GroupTree, groupId: string): string[] {
  const chain: string[] = []
  const seen = new Set<string>()
  let current: string | null = tree.parentOf.has(groupId) ? groupId : null
  while (current !== null && !seen.has(current)) {
    seen.add(current)
    chain.push(current)
    current = tree.parentOf.get(current) ?? null
    if (chain.length > 256) break
  }
  return chain.reverse()
}
