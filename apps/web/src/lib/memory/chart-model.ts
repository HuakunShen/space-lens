import type { MemorySnapshot, ProcessRow } from '@space-lens/contract'
import { buildForest, type TreeNode } from '@space-lens/web-ui/lib/memory/tree'

export interface Branch {
  id: string
  name: string
  bytes: number
  ownBytes: number | null
  pids: number[]
  children: Branch[]
  kind: 'process' | 'group' | 'self' | 'other'
  omitted?: number
}

export function processLabel(row: ProcessRow | undefined, fallback: string): string {
  const text = `${row?.name ?? ''} ${row?.command ?? ''}`.toLowerCase()
  if (text.includes('context7')) return 'Context7'
  if (text.includes('codegraph')) return 'CodeGraph'
  if (text.includes('gk-alpha')) return 'gk-alpha'
  // Generic runtimes do not identify the program they execute.
  if (/^(node|npm|npx)$/i.test(fallback)) return `${fallback} · PID ${row?.pid ?? '?'}`
  return fallback
}

/** Make a bounded visual slice; subtree totals come from the complete snapshot. */
export function buildMemorySlice(
  snapshot: MemorySnapshot,
  focusPid: number | null,
  depth: number,
  merge: boolean,
): { branches: Branch[]; total: number; focus: TreeNode | null } {
  const forest = buildForest(snapshot)
  const rows = new Map(snapshot.processes.map((row) => [row.pid, row]))
  const focus = focusPid === null ? null : (forest.byPid.get(focusPid) ?? null)
  let budget = 180
  const branch = (nodes: TreeNode[], level: number): Branch[] => {
    const buckets = new Map<string, TreeNode[]>()
    for (const node of nodes) {
      const name = processLabel(rows.get(node.pid), node.name)
      const key = merge ? name : `${name}:${node.pid}`
      const items = buckets.get(key) ?? []
      items.push(node)
      buckets.set(key, items)
    }
    const sorted = [...buckets.values()].sort((a, b) => sum(b) - sum(a))
    const shown: Branch[] = []
    let omittedBytes = 0,
      omitted = 0
    const omittedPids: number[] = []
    for (const nodes of sorted) {
      const bytes = sum(nodes)
      if (bytes <= 0) continue
      if (shown.length >= 12 || budget <= 0) {
        omittedBytes += bytes
        omitted += nodes.length
        omittedPids.push(...nodes.map((n) => n.pid))
        continue
      }
      budget--
      const first = nodes[0]!
      const name = processLabel(rows.get(first.pid), first.name)
      const children =
        level < depth
          ? branch(
              nodes.flatMap((node) => node.children),
              level + 1,
            )
          : []
      const own = nodes.reduce((bytes, node) => bytes + (node.footprintBytes ?? node.residentBytes ?? 0), 0)
      if (children.length > 0 && own > 0)
        children.unshift({
          id: `self:${nodes.map((node) => node.pid).join(':')}`,
          name: 'Self',
          bytes: own,
          ownBytes: first.footprintBytes,
          pids: nodes.map((node) => node.pid),
          children: [],
          kind: 'self',
        })
      shown.push({
        id: nodes.map((n) => n.pid).join(':'),
        name,
        bytes,
        ownBytes: nodes.length === 1 ? first.footprintBytes : null,
        pids: nodes.map((n) => n.pid),
        children,
        kind: nodes.length === 1 ? 'process' : 'group',
      })
    }
    if (omittedBytes > 0)
      shown.push({
        id: `other:${level}:${nodes[0]?.pid ?? 0}`,
        name: 'Other',
        bytes: omittedBytes,
        ownBytes: null,
        pids: omittedPids,
        children: [],
        kind: 'other',
        omitted,
      })
    return shown
  }
  if (focus) {
    const branches = branch(focus.children, 1)
    const own = focus.footprintBytes ?? focus.residentBytes ?? 0
    if (own > 0)
      branches.unshift({
        id: `self:${focus.pid}`,
        name: 'Self',
        bytes: own,
        ownBytes: own,
        pids: [focus.pid],
        children: [],
        kind: 'self',
      })
    return { branches, total: focus.subtreeBytes, focus }
  }
  // Snapshot groups partition process rows. Do not add subtree totals here:
  // a child may belong to a different application group.
  const grouped = new Map<string, { name: string; bytes: number; pids: number[]; mainPids: Set<number> }>()
  const groupById = new Map(snapshot.groups.map((g) => [g.groupId, g]))
  for (const row of snapshot.processes) {
    const group = groupById.get(row.groupId)
    const name = processLabel(row, group?.name ?? row.name)
    const key = merge ? name : row.groupId
    const item = grouped.get(key) ?? { name, bytes: 0, pids: [], mainPids: new Set<number>() }
    item.bytes += row.footprintBytes ?? row.residentBytes ?? 0
    item.pids.push(row.pid)
    item.mainPids.add(name === (group?.name ?? row.name) ? (group?.mainPid ?? row.pid) : row.pid)
    grouped.set(key, item)
  }
  const all = [...grouped.values()].filter((g) => g.bytes > 0).sort((a, b) => b.bytes - a.bytes)
  // Project the actual parent links onto each application group. A process
  // contributes only its own footprint to its group, even when descendants
  // belong to another group; this keeps every ring's accounting consistent.
  const branches: Branch[] = all.slice(0, 30).map((g) => {
    const members = new Set(g.pids)
    const projected = new Map<number, TreeNode>()
    for (const pid of g.pids) {
      const node = forest.byPid.get(pid)
      if (node)
        projected.set(pid, { ...node, children: [], subtreeBytes: node.footprintBytes ?? node.residentBytes ?? 0 })
    }
    const roots: TreeNode[] = []
    for (const node of projected.values()) {
      const parent = node.ppid === null ? undefined : projected.get(node.ppid)
      // buildForest has already broken cycles; only retain links it accepted.
      const originalParent = node.ppid === null ? undefined : forest.byPid.get(node.ppid)
      if (parent && originalParent?.children.some((child) => child.pid === node.pid)) parent.children.push(node)
      else roots.push(node)
    }
    const addBytes = (node: TreeNode): number => {
      for (const child of node.children) node.subtreeBytes += addBytes(child)
      return node.subtreeBytes
    }
    for (const root of roots) addBytes(root)
    let children: Branch[] = []
    if (depth > 1) {
      if (roots.length === 1) {
        const root = roots[0]!
        children = branch(root.children, 2)
        const own = root.footprintBytes ?? root.residentBytes ?? 0
        if (children.length && own > 0)
          children.unshift({
            id: `self:${root.pid}`,
            name: 'Self',
            bytes: own,
            ownBytes: root.footprintBytes,
            pids: [root.pid],
            children: [],
            kind: 'self',
          })
      } else {
        children = branch(roots, 2)
      }
    }
    return {
      id: `group:${g.name}:${[...g.mainPids][0]}`,
      name: g.name,
      bytes: g.bytes,
      ownBytes: null,
      pids: [...g.mainPids].filter((pid) => members.has(pid)),
      children,
      kind: 'group',
      omitted: g.pids.length,
    }
  })
  const rest = all.slice(30)
  if (rest.length)
    branches.push({
      id: 'other:root',
      name: 'Other',
      bytes: rest.reduce((n, g) => n + g.bytes, 0),
      ownBytes: null,
      pids: rest.flatMap((g) => [...g.mainPids]),
      children: [],
      kind: 'other',
      omitted: rest.length,
    })
  return { branches, total: all.reduce((n, g) => n + g.bytes, 0), focus: null }
}
function sum(nodes: TreeNode[]): number {
  return nodes.reduce((total, node) => total + node.subtreeBytes, 0)
}
