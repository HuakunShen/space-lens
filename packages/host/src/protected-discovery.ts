import type { DiscoveryKind } from '@space-lens/contract'
import { basename, dirname, join } from 'node:path'
import type { DiscoveryView } from './discovery.ts'
import { nodeIdOf, type ScanIndex } from './scan-index.ts'

/**
 * Size-descending, then path order. Paths are rebuilt only for rows whose
 * size ties, so large-file lists never hold one path string per file.
 */
function sortRows(index: ScanIndex, rows: number[]): Int32Array {
  const order = Int32Array.from(rows)
  order.sort((a, b) => index.sizeOf(b) - index.sizeOf(a) || a - b)
  for (let start = 0; start < order.length; ) {
    const size = index.sizeOf(order[start])
    let end = start + 1
    while (end < order.length && index.sizeOf(order[end]) === size) end += 1
    if (end - start > 1) {
      const run = Array.from(order.subarray(start, end), (row) => ({ row, path: index.pathOf(row) }))
      run.sort((a, b) => a.path.localeCompare(b.path) || a.row - b.row)
      for (let offset = 0; offset < run.length; offset += 1) order[start + offset] = run[offset].row
    }
    start = end
  }
  return order
}

/** Classify already protected report metadata; no filesystem probes or rescans. */
export async function protectedDiscovery(index: ScanIndex): Promise<DiscoveryView> {
  const python = new Map([
    ['__pycache__', 'Python bytecode'],
    ['.pytest_cache', 'pytest cache'],
    ['.mypy_cache', 'mypy cache'],
    ['.ruff_cache', 'Ruff cache'],
  ])
  const js = new Set(['.next', '.nuxt', '.turbo', '.parcel-cache', '.svelte-kit'])
  const isLocalFile = (path: string) => {
    const row = index.find(nodeIdOf(path))
    return row !== -1 && index.isDirectory(row) === false && index.scanStateOf(row) === 'complete'
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
  const rows: Record<DiscoveryKind, number[]> = { 'large-files': [], caches: [], gitignored: [] }
  const cacheCategories = new Map<number, string>()
  for (let row = 0; row < index.count; row += 1) {
    if ((row + 1) % 1000 === 0) await new Promise<void>((yieldTurn) => setImmediate(yieldTurn))
    if (index.scanStateOf(row) === 'skipped' || index.parentOf(row) === -1) continue
    if (index.isIgnored(row)) rows.gitignored.push(row)
    if (index.isDirectory(row) === false) {
      rows['large-files'].push(row)
      continue
    }
    const name = index.baseNameOf(row)
    let category = python.get(name)
    if (category === undefined && name !== 'node_modules' && name !== 'target' && !js.has(name)) continue
    const path = index.pathOf(row)
    if (name === 'node_modules' && isLocalFile(join(dirname(path), 'package.json'))) category = 'Node dependencies'
    if (name === 'target' && isLocalFile(join(dirname(path), 'Cargo.toml'))) category = 'Rust build output'
    if (js.has(name) && isLocalFile(join(dirname(path), 'package.json'))) category = 'JavaScript build cache'
    if (category !== undefined && !inEnvironment(path)) {
      rows.caches.push(row)
      cacheCategories.set(row, category)
    }
  }
  for (const kind of ['caches', 'gitignored'] as const) {
    // Drop candidates nested under another directory candidate of the same kind.
    const selected = new Uint8Array(index.count)
    for (const row of rows[kind]) if (index.isDirectory(row) === true) selected[row] = 1
    const rootNested = new Map<number, boolean>()
    const nestedAboveRoot = (root: number) => {
      let nested = rootNested.get(root)
      if (nested === undefined) {
        nested = false
        let cursor = index.pathOf(root)
        for (let parent = dirname(cursor); parent !== cursor; cursor = parent, parent = dirname(cursor)) {
          const row = index.find(nodeIdOf(parent))
          if (row !== -1 && selected[row] === 1) {
            nested = true
            break
          }
        }
        rootNested.set(root, nested)
      }
      return nested
    }
    rows[kind] = rows[kind].filter((row) => {
      let cursor = index.parentOf(row)
      for (;;) {
        if (selected[cursor] === 1) return false
        const parent = index.parentOf(cursor)
        if (parent === -1) return !nestedAboveRoot(cursor)
        cursor = parent
      }
    })
  }
  const order = {} as Record<DiscoveryKind, Int32Array>
  const cumulative = {} as Record<DiscoveryKind, Float64Array>
  for (const kind of Object.keys(rows) as DiscoveryKind[]) {
    order[kind] = sortRows(index, rows[kind])
    rows[kind] = []
    const totals = new Float64Array(order[kind].length + 1)
    for (let position = 0; position < order[kind].length; position += 1)
      totals[position + 1] = totals[position] + index.sizeOf(order[kind][position])
    cumulative[kind] = totals
  }
  const categoryOf = (kind: DiscoveryKind, row: number) =>
    kind === 'large-files' ? 'Large file' : kind === 'gitignored' ? 'Gitignored' : cacheCategories.get(row)!
  return {
    count: (kind) => order[kind].length,
    sizeAt: (kind, position) => index.sizeOf(order[kind][position]),
    itemAt: (kind, position) => {
      const row = order[kind][position]
      return {
        node: index.summarize(row),
        parentId: index.idOf(index.parentOf(row)),
        category: categoryOf(kind, row),
        isDirectory: index.isDirectory(row) === true,
      }
    },
    cumulativeSize: (kind, count) => cumulative[kind][count],
  }
}
