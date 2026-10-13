import type { AppAuditLocation, AppAuditReport, ScanStatus, TreeNodeSummary } from '@space-lens/contract'

/**
 * V3 app-scan helpers shared by every frontend surface.
 *
 * Virtual-root conventions (pinned, plan §3.2):
 * - virtualRootId = `approot_<24hex>`, virtualPath = `app-storage:<24hex>`
 * - legacy drawer ids (`approot:`, `apploc:`, `appbundle:`) are recognized
 *   for display guards only — the engine never emits them.
 * - bundle leaf id = `appbundle_<24hex>`, depth 1, audit-time size.
 * - location roots keep `node_id_of(canonicalPath)` ids, depth shifted to 1.
 */

export function isVirtualAppPath(path: string | null | undefined): boolean {
  return typeof path === 'string' && path.startsWith('app-storage:')
}

export function isVirtualAppId(id: string | null | undefined): boolean {
  return (
    typeof id === 'string' &&
    (id.startsWith('approot_') || id.startsWith('approot:') || isVirtualAppPath(id))
  )
}

export function isAppBundleNode(node: Pick<TreeNodeSummary, 'id' | 'path'>): boolean {
  return node.id.startsWith('appbundle_') || node.id.startsWith('appbundle:')
}

/** Truncation buckets (`<parent>:omitted`) — actionable only by drilling in. */
export function isOmittedBucket(node: Pick<TreeNodeSummary, 'id'>): boolean {
  return node.id.endsWith(':omitted')
}

/**
 * A12 accounting leaves: the engine represents a directory's own allocation
 * (measured size minus the sum of emitted children) as an explicit
 * non-actionable child so every chart mode reconciles with the virtual
 * total. Distinct from truncation (`:omitted`) buckets by id suffix and by
 * an optional host flag (forward-compatible: the pinned TreeNodeSummary is
 * unchanged, so the flag is read loosely).
 */
export function isAccountingLeaf(node: Pick<TreeNodeSummary, 'id' | 'name'>): boolean {
  if (node.id.endsWith(':accounting') || node.id.endsWith(':own')) return true
  const flag = (node as unknown as Record<string, unknown>).accounting
  return flag === true
}

export function isAppScanStatus(status: Pick<ScanStatus, 'app'> | null | undefined): boolean {
  return Boolean(status?.app)
}

export type CleanupEligibility = 'cleanable' | 'contents-only' | 'refused'

const ELIGIBILITY_VALUES: ReadonlySet<string> = new Set(['cleanable', 'contents-only', 'refused'])

function hostEligibility(node: TreeNodeSummary): CleanupEligibility | null {
  // A8: ONE host-computed predicate surfaced via the slice/status wire. The
  // pinned TreeNodeSummary is unchanged (§3.1), so the field is read loosely
  // — the engine lane adds it additively; when present it wins everywhere.
  const record = node as unknown as Record<string, unknown>
  for (const key of ['eligibility', 'cleanupEligibility', 'cleanup', 'collectable', 'collectible']) {
    const value = record[key]
    if (typeof value === 'string' && ELIGIBILITY_VALUES.has(value)) return value as CleanupEligibility
  }
  return null
}

/**
 * A8 single eligibility consumption point. Every selection entry point —
 * collect(), bulk selection, child-list controls, discovery controls, chart
 * actions — calls this (via canCollectNode); no call site keeps its own
 * partial/skipped heuristic.
 *
 * Precedence: structural refusals (virtual root, bundle, accounting leaf,
 * truncation bucket) → host predicate (A1 evidence, when the engine
 * supplies it) → legacy scanState fallback for path scans.
 */
export function eligibilityOf(node: TreeNodeSummary): CleanupEligibility {
  if (isVirtualAppPath(node.path) || isVirtualAppId(node.id)) return 'refused'
  if (isAppBundleNode(node)) return 'refused'
  if (isAccountingLeaf(node) || isOmittedBucket(node)) return 'refused'
  const host = hostEligibility(node)
  if (host !== null) return host
  if (node.scanState === 'skipped' || node.scanState === 'partial') return 'refused'
  return 'cleanable'
}

/** Whole-folder staging is allowed only for `cleanable` nodes. */
export function canCollectNode(node: TreeNodeSummary): boolean {
  return eligibilityOf(node) === 'cleanable'
}

/** Human reason for a disabled collect control. */
export function collectRefusalReason(node: TreeNodeSummary): string | null {
  if (isVirtualAppPath(node.path) || isVirtualAppId(node.id)) return 'The app virtual root cannot be cleaned up'
  if (isAppBundleNode(node)) return 'App bundles cannot be cleaned up; uninstall is out of scope'
  if (isAccountingLeaf(node)) return "This folder's own files are accounted here; open the folder to select items"
  if (isOmittedBucket(node)) return 'Grouped items — open the parent folder to explore'
  const host = hostEligibility(node)
  if (host === 'contents-only') return 'Folder-level cleanup unavailable here — select its contents instead'
  if (host === 'refused') return 'Not cleanable in this scan'
  if (node.scanState === 'skipped') return `Not scanned · ${node.skipReason ?? 'unavailable'}`
  if (node.scanState === 'partial') return 'Partially scanned — contents-only cleanup'
  return null
}

/**
 * Tree-ancestry check for app scans (plan §5): path-prefix dimming is wrong
 * under a virtual root (locations are disjoint real paths), so charts dim
 * by id parent-chain instead. `parentById` maps child id → parent id for the
 * current slice; pass null to fall back to path-prefix logic (path scans).
 */
export function isAncestorById(
  ancestorId: string,
  descendantId: string,
  parentById: Map<string, string> | null,
): boolean {
  if (ancestorId === descendantId) return true
  if (!parentById) return false
  let current = parentById.get(descendantId)
  while (current !== undefined) {
    if (current === ancestorId) return true
    current = parentById.get(current)
  }
  return false
}

/** Builds the child→parent id map for one slice tree (id-based, no paths). */
export function parentIdMap(tree: { id: string; children?: readonly ParentNode[] } | null): Map<string, string> {
  const map = new Map<string, string>()
  if (!tree) return map
  const walk = (node: ParentNode, parent: string | null): void => {
    if (parent !== null) map.set(node.id, parent)
    for (const child of node.children ?? []) walk(child, node.id)
  }
  walk(tree as ParentNode, null)
  return map
}

interface ParentNode {
  id: string
  children?: readonly ParentNode[]
}

export interface AuditAnnotation {
  label: string | null
  classification: AppAuditLocation['classification']
  comment: string | null
  purgeable: boolean
  source: AppAuditLocation['source']
}

/**
 * A11 inspector join: for a depth-1 location node (child of the virtual
 * root), look up the pinned per-scan report's catalog metadata. Returns null
 * for the virtual root itself, the bundle leaf, and deeper nodes.
 */
export function annotationForNode(
  node: Pick<TreeNodeSummary, 'id' | 'path' | 'depth'>,
  isDepthOneChildOfVirtualRoot: boolean,
  report: AppAuditReport | null,
  appId: string | null,
): AuditAnnotation | null {
  if (!report || !appId || !isDepthOneChildOfVirtualRoot) return null
  if (isVirtualAppPath(node.path) || isAppBundleNode(node)) return null
  const entry = report.apps.find((app) => app.id === appId)
  const location = entry?.locations.find((loc) => loc.path === node.path)
  if (!location) return null
  return {
    label: location.label,
    classification: location.classification,
    comment: location.comment,
    purgeable: location.purgeable,
    source: location.source,
  }
}

/** A10: synthetic identifiers must never reach reveal/rescan/filesystem. */
export function assertRealPath(path: string, action: string): void {
  if (isVirtualAppPath(path)) throw new Error(`${action} refuses synthetic app identifiers (${path})`)
}

/**
 * A11 client-side app-scan history entry. The pinned contract
 * RecentAppScan (appId/appName/auditId/lastScannedAt) is the wire shape;
 * provenance (generatedAt) and staleness ride alongside it so annotations
 * + stale badges survive reload. Storage validation picks the pinned keys
 * through RecentAppScanSchema and keeps these extras when present.
 */
export interface RecentAppEntry {
  appId: string
  appName: string
  auditId: string
  lastScannedAt: string
  generatedAt?: string | null
  stale?: boolean
}
