import type { DiscoveryItem, DiscoveryKind } from '@space-lens/contract'
import type { DirectoryNode } from 'space-lens'

export interface DiscoveredEntry {
  id: string
  node: DirectoryNode
  parent: string | null
  childIds: string[]
  depth: number
  fingerprint: { size: number; mtimeMs: number }
}

export interface DiscoverySnapshot {
  entries: DiscoveredEntry[]
  items: Record<DiscoveryKind, DiscoveryItem[]>
  cumulativeSizes: Record<DiscoveryKind, number[]>
}

/**
 * Plain JavaScript source is embedded deliberately: serializing transpiled
 * functions captures loader/bundler helpers (for example tsx keepNames), which
 * are unavailable in an isolated worker. This also keeps bundled hosts free
 * of a separate worker file that would need to be copied beside the bundle.
 */
export const DISCOVERY_BOOTSTRAP = `
/** Self-contained so its compiled function can run inside the scan worker. */
function inspectDiscovery(engine, fs, path, crypto, roots) {
  const within = (candidate, root) =>
    candidate === root || candidate.startsWith(root.endsWith(path.sep) ? root : \`\${root}\${path.sep}\`)
  const idOf = (value) => crypto.createHash('sha1').update(value).digest('hex').slice(0, 24)
  const safeStat = (value) => {
    try {
      const stats = fs.lstatSync(value)
      if (stats.isSymbolicLink() || fs.realpathSync(value) !== value || !roots.some((root) => within(value, root)))
        return null
      return stats
    } catch {
      return null
    }
  }
  const tree = engine.scanDirectory({
    directories: roots,
    fullPath: true,
    ignoreHidden: false,
    respectGitignore: false,
    followSymlinks: false,
  })
  const ignoredTree = engine.scanDirectory({
    directories: roots,
    fullPath: true,
    ignoreHidden: false,
    respectGitignore: true,
    ignoredMode: 'summarize',
    followSymlinks: false,
  })
  const ignoredPaths = []
  const collectIgnored = (node) => {
    if (node.ignored) ignoredPaths.push(node.path)
    else for (const child of node.children) collectIgnored(child)
  }
  for (const root of ignoredTree) collectIgnored(root)
  const ignoredSet = new Set(ignoredPaths)
  const isIgnored = (value) => {
    let cursor = value
    for (;;) {
      if (ignoredSet.has(cursor)) return true
      const parent = path.dirname(cursor)
      if (parent === cursor) return false
      cursor = parent
    }
  }
  const entries = []
  const byPath = new Map()
  const directories = new Set()
  const regularFiles = new Set()
  const walk = (node, parent, depth) => {
    const stats = safeStat(node.path)
    if (stats === null) return null
    const id = idOf(node.path)
    const entry = {
      id,
      node: { ...node, children: [], ignored: isIgnored(node.path) },
      parent,
      depth,
      childIds: [],
      fingerprint: { size: stats.size, mtimeMs: stats.mtimeMs },
    }
    entries.push(entry)
    byPath.set(node.path, entry)
    if (stats.isDirectory()) directories.add(node.path)
    if (stats.isFile()) regularFiles.add(node.path)
    entry.childIds = node.children.map((child) => walk(child, id, depth + 1)).filter((id) => id !== null)
    return id
  }
  for (const root of tree) walk(root, null, 0)
  const summary = (entry) => ({
    id: entry.id,
    name: path.basename(entry.node.path),
    path: entry.node.path,
    size: entry.node.size,
    depth: entry.depth,
    ignored: entry.node.ignored,
    collapsed: false,
    hasChildren: entry.childIds.length > 0,
    childCount: entry.childIds.length,
  })
  const item = (entry, category) => ({
    node: summary(entry),
    parentId: entry.parent,
    category,
    isDirectory: directories.has(entry.node.path),
  })
  const hasFile = (directory, name) => safeStat(path.join(directory, name))?.isFile() === true
  const pythonEnvironment = (value) => {
    let cursor = path.dirname(value)
    for (;;) {
      if (['.venv', 'venv', 'env'].includes(path.basename(cursor)) || hasFile(cursor, 'pyvenv.cfg')) return true
      if (roots.includes(cursor)) return false
      const parent = path.dirname(cursor)
      if (parent === cursor) return false
      cursor = parent
    }
  }
  const pythonCategories = new Map([
    ['__pycache__', 'Python bytecode'],
    ['.pytest_cache', 'pytest cache'],
    ['.mypy_cache', 'mypy cache'],
    ['.ruff_cache', 'Ruff cache'],
  ])
  const jsNames = new Set(['.next', '.nuxt', '.turbo', '.parcel-cache', '.svelte-kit'])
  const cacheCategory = (entry) => {
    const value = entry.node.path
    if (!directories.has(value) || entry.parent === null) return null
    const name = path.basename(value)
    if (name !== 'node_modules' && name !== 'target' && !pythonCategories.has(name) && !jsNames.has(name)) return null
    if (pythonEnvironment(value)) return null
    const parent = path.dirname(value)
    if (name === 'node_modules' && hasFile(parent, 'package.json')) return 'Node dependencies'
    if (name === 'target' && hasFile(parent, 'Cargo.toml')) return 'Rust build output'
    if (pythonCategories.has(name)) return pythonCategories.get(name)
    if (jsNames.has(name) && hasFile(parent, 'package.json')) return 'JavaScript build cache'
    return null
  }
  const prune = (items) => {
    const sorted = items.sort(
      (a, b) => a.node.path.length - b.node.path.length || a.node.path.localeCompare(b.node.path),
    )
    const kept = []
    const keptPaths = new Set()
    const keptDirectories = new Set()
    for (const candidate of sorted) {
      if (keptPaths.has(candidate.node.path)) continue
      let cursor = path.dirname(candidate.node.path)
      let nested = false
      for (;;) {
        if (keptDirectories.has(cursor)) {
          nested = true
          break
        }
        const parent = path.dirname(cursor)
        if (parent === cursor) break
        cursor = parent
      }
      if (nested) continue
      kept.push(candidate)
      keptPaths.add(candidate.node.path)
      if (candidate.isDirectory) keptDirectories.add(candidate.node.path)
    }
    return kept
  }
  const caches = []
  const files = []
  for (const entry of entries) {
    if (regularFiles.has(entry.node.path)) files.push(item(entry, 'Large file'))
    const category = cacheCategory(entry)
    if (category !== null) caches.push(item(entry, category))
  }
  const ignored = ignoredPaths
    .map((value) => byPath.get(value))
    .filter((entry) => entry !== undefined)
    .filter((entry) => entry.parent !== null)
    .map((entry) => item(entry, 'Gitignored'))
  const items = { 'large-files': files, caches: prune(caches), gitignored: prune(ignored) }
  const cumulativeSizes = {}
  for (const kind of Object.keys(items)) {
    items[kind].sort((a, b) => b.node.size - a.node.size || a.node.path.localeCompare(b.node.path))
    const sums = [0]
    for (const candidate of items[kind]) sums.push(sums[sums.length - 1] + candidate.node.size)
    cumulativeSizes[kind] = sums
  }
  return { entries, items, cumulativeSizes }
}

;(async () => {
  const { parentPort, workerData } = await import('node:worker_threads')
  try {
    const loaded = await import(workerData.enginePath)
    const result = inspectDiscovery(
      loaded.default ?? loaded,
      await import('node:fs'),
      await import('node:path'),
      await import('node:crypto'),
      workerData.roots,
    )
    parentPort.postMessage({ type: 'done', result })
  } catch (error) {
    parentPort.postMessage({ type: 'error', message: String(error && error.message ? error.message : error) })
  }
})()
`
