import type {
  AppAuditClassification,
  AppAuditEntry,
  AppAuditLocation,
  AppAuditReport,
  TreeSliceNode,
} from '@space-lens/contract'

const CLASSIFICATION_LABELS: Record<AppAuditClassification, string> = {
  application: 'Application',
  cache: 'Cache',
  versions: 'Old versions',
  logs: 'Logs',
  're-downloadable': 'Re-downloadable',
  'user-data': 'User data',
  support: 'Support',
}

const CLASSIFICATION_BADGE_TOKENS: Record<AppAuditClassification, string> = {
  application: 'app-audit-badge-application',
  cache: 'app-audit-badge-cache',
  versions: 'app-audit-badge-versions',
  logs: 'app-audit-badge-logs',
  're-downloadable': 'app-audit-badge-redownloadable',
  'user-data': 'app-audit-badge-user-data',
  support: 'app-audit-badge-support',
}

/** Stable per-class token for the classification badge; the component pairs it
 * with the shared badge layout classes. */
export function classificationBadge(classification: AppAuditClassification): string {
  return CLASSIFICATION_BADGE_TOKENS[classification]
}

/** Title-cased display label for each audit classification. */
export function classificationLabel(classification: AppAuditClassification): string {
  return CLASSIFICATION_LABELS[classification]
}

/** Sum of sizes of locations flagged purgeable by their classification. */
export function purgeableTotal(locations: Pick<AppAuditLocation, 'purgeable' | 'size'>[]): number {
  return locations.reduce((total, location) => (location.purgeable ? total + location.size : total), 0)
}

/** First-letter avatar text for an app name; tolerates blank names. */
export function initialsFor(name: string): string {
  const first = name.trim().charAt(0)
  return first === '' ? '?' : first.toUpperCase()
}

/** Client-side name/bundleId filter for the app storage search box. */
export function matchesQuery(entry: Pick<AppAuditEntry, 'name' | 'bundleId'>, query: string): boolean {
  const needle = query.trim().toLowerCase()
  if (needle === '') return true
  if (entry.name.toLowerCase().includes(needle)) return true
  return (entry.bundleId ?? '').toLowerCase().includes(needle)
}

/** The largest location in an entry, or null when it has none. */
export function largestLocation(entry: Pick<AppAuditEntry, 'locations'>): AppAuditLocation | null {
  let largest: AppAuditLocation | null = null
  for (const location of entry.locations) {
    if (largest === null || location.size > largest.size) largest = location
  }
  return largest
}

/** Last path segment, tolerating trailing slashes; falls back to the full path. */
export function basename(path: string): string {
  const trimmed = path.replace(/\/+$/, '')
  if (trimmed === '') return path
  const slash = trimmed.lastIndexOf('/')
  const base = slash >= 0 ? trimmed.slice(slash + 1) : trimmed
  return base === '' ? trimmed : base
}

function comparableAuditPath(path: string): string {
  const normalized = path.replace(/\/+$/, '')
  return normalized === '' ? '/' : normalized
}

/**
 * True when `path` sits strictly inside any staged path. Staging a parent
 * folder covers its itemized children (audit sizes are already partitioned,
 * so the child must not be staged separately). A path never covers itself.
 */
export function isPathCovered(path: string, staged: Iterable<string>): boolean {
  const child = comparableAuditPath(path)
  for (const candidate of staged) {
    const ancestor = comparableAuditPath(candidate)
    if (child !== ancestor && child.startsWith(ancestor === '/' ? '/' : `${ancestor}/`)) return true
  }
  return false
}

/** Keeps only staged paths still present in the latest report, preserving order. */
export function pruneStaged(paths: string[], valid: Iterable<string>): string[] {
  const allowed = new Set(valid)
  return paths.filter((path) => allowed.has(path))
}

/**
 * Re-validates the per-app staged map against a fresh report: apps that
 * vanished are dropped, and so are paths the report no longer lists.
 * A null report preserves the map (errors never blank the report either).
 */
export function validateStagedAgainstReport(
  staged: Map<string, Set<string>>,
  report: AppAuditReport | null,
): Map<string, Set<string>> {
  if (!report) return new Map(staged)
  const validByApp = new Map(report.apps.map((app) => [app.id, new Set(app.locations.map((l) => l.path))]))
  const next = new Map<string, Set<string>>()
  for (const [appId, paths] of staged) {
    const valid = validByApp.get(appId)
    if (!valid) continue
    const kept = new Set([...paths].filter((path) => valid.has(path)))
    if (kept.size > 0) next.set(appId, kept)
  }
  return next
}

/**
 * Synthesizes the depth-1 app-rooted chart slice the drawer paints: a
 * virtual app root with one leaf branch per audited location plus the
 * `.app` bundle as a first-class `application` segment. Sizes are the
 * already-partitioned audit sizes (deepest-first measurement), so no
 * deep-scan read arm is needed for a correct partition render.
 */
export function appTreeSlice(entry: AppAuditEntry): TreeSliceNode {
  const fallbackPath = entry.locations[0]?.path ?? '/'
  const children: TreeSliceNode[] = []
  if (entry.installedSize > 0) {
    children.push({
      id: `appbundle:${entry.id}`,
      name: 'App bundle',
      path: entry.appPath ?? fallbackPath,
      size: entry.installedSize,
      depth: 1,
      ignored: false,
      collapsed: false,
      hasChildren: false,
      childCount: 0,
      isDirectory: true,
      scanState: 'complete',
      children: [],
      omittedBytes: 0,
      omittedCount: 0,
    })
  }
  for (const location of entry.locations) {
    children.push({
      id: `apploc:${location.path}`,
      name: location.label ?? basename(location.path),
      path: location.path,
      size: location.size,
      depth: 1,
      ignored: false,
      collapsed: false,
      hasChildren: false,
      childCount: 0,
      children: [],
      omittedBytes: 0,
      omittedCount: 0,
    })
  }
  return {
    id: `approot:${entry.id}`,
    name: entry.name,
    path: entry.appPath ?? fallbackPath,
    size: entry.totalSize,
    depth: 0,
    ignored: false,
    collapsed: false,
    hasChildren: children.length > 0,
    childCount: children.length,
    isDirectory: true,
    scanState: 'complete',
    children,
    omittedBytes: 0,
    omittedCount: 0,
  }
}
