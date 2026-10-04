import type { DiscoveryItem, DiscoveryKind, LocalScanNode, TreeNodeSummary } from '@space-lens/contract'
import { basename, dirname, join } from 'node:path'
import type { DiscoverySnapshot } from './discovery.ts'

interface ProtectedEntry {
  id: string
  node: { path: string; name: string; size: number; ignored: boolean } & Partial<
    Pick<LocalScanNode, 'scanState' | 'isDirectory'>
  >
  parent: string | null
  childIds: string[]
  depth: number
}

/** Classify already protected report metadata; no filesystem probes or rescans. */
export async function protectedDiscovery<T extends ProtectedEntry>(
  entries: Iterable<T>,
  summarize: (entry: T) => TreeNodeSummary,
): Promise<DiscoverySnapshot> {
  const all = [...entries]
  const byPath = new Map(all.map((entry) => [entry.node.path, entry]))
  const python = new Map([
    ['__pycache__', 'Python bytecode'],
    ['.pytest_cache', 'pytest cache'],
    ['.mypy_cache', 'mypy cache'],
    ['.ruff_cache', 'Ruff cache'],
  ])
  const js = new Set(['.next', '.nuxt', '.turbo', '.parcel-cache', '.svelte-kit'])
  const isLocalFile = (path: string) => {
    const node = byPath.get(path)?.node
    return node?.isDirectory === false && node.scanState === 'complete'
  }
  const inEnvironment = (path: string) => {
    let cursor = dirname(path)
    for (;;) {
      if (['.venv', 'venv', 'env'].includes(basename(cursor)) || isLocalFile(join(cursor, 'pyvenv.cfg'))) return true
      const parent = dirname(cursor)
      if (parent === cursor) return false
      cursor = parent
    }
  }
  const items: Record<DiscoveryKind, DiscoveryItem[]> = { 'large-files': [], caches: [], gitignored: [] }
  let count = 0
  for (const entry of all) {
    if (++count % 1000 === 0) await new Promise<void>((yieldTurn) => setImmediate(yieldTurn))
    if (entry.node.scanState === 'skipped' || entry.parent === null) continue
    const item = (category: string): DiscoveryItem => ({
      node: summarize(entry),
      parentId: entry.parent,
      category,
      isDirectory: entry.node.isDirectory === true,
    })
    if (entry.node.ignored) items.gitignored.push(item('Gitignored'))
    if (entry.node.isDirectory === false) {
      items['large-files'].push(item('Large file'))
      continue
    }
    const name = basename(entry.node.path)
    let category = python.get(name)
    if (name === 'node_modules' && isLocalFile(join(dirname(entry.node.path), 'package.json')))
      category = 'Node dependencies'
    if (name === 'target' && isLocalFile(join(dirname(entry.node.path), 'Cargo.toml'))) category = 'Rust build output'
    if (js.has(name) && isLocalFile(join(dirname(entry.node.path), 'package.json'))) category = 'JavaScript build cache'
    if (category !== undefined && !inEnvironment(entry.node.path)) items.caches.push(item(category))
  }
  for (const kind of ['caches', 'gitignored'] as const) {
    const directories = new Set(items[kind].filter((item) => item.isDirectory).map((item) => item.node.path))
    items[kind] = items[kind].filter((item) => {
      let cursor = dirname(item.node.path)
      for (;;) {
        if (directories.has(cursor)) return false
        const parent = dirname(cursor)
        if (parent === cursor) return true
        cursor = parent
      }
    })
  }
  const cumulativeSizes = {} as Record<DiscoveryKind, number[]>
  for (const kind of Object.keys(items) as DiscoveryKind[]) {
    items[kind].sort((a, b) => b.node.size - a.node.size || a.node.path.localeCompare(b.node.path))
    const totals = [0]
    for (const item of items[kind]) totals.push(totals[totals.length - 1] + item.node.size)
    cumulativeSizes[kind] = totals
  }
  return { entries: [], items, cumulativeSizes }
}
