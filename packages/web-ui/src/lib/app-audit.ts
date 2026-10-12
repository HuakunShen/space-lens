import type { AppAuditClassification, AppAuditEntry, AppAuditLocation } from '@space-lens/contract'

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
