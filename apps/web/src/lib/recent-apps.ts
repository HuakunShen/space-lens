import { RecentAppScanSchema, type RecentAppScan } from '@space-lens/contract'
import type { RecentAppEntry } from '@space-lens/web-ui/lib/app-scan'

/**
 * A11 client-side app-scan history. The pinned RecentAppScan shape
 * (appId/appName/auditId/lastScannedAt, localStorage key
 * spacelens.recentAppScans, max 5) is validated exactly; provenance
 * (generatedAt) and staleness ride alongside it so annotations + stale
 * badges survive reload. Kept separate from spacelens.recentScans.
 */
export const RECENT_APP_KEY = 'spacelens.recentAppScans'
const RECENT_APP_MAX = 5

function stored(): unknown[] {
  try {
    const raw = window.localStorage.getItem(RECENT_APP_KEY)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    return Array.isArray(parsed) ? parsed : []
  } catch {
    return []
  }
}

function coerce(raw: unknown): RecentAppEntry | null {
  if (typeof raw !== 'object' || raw === null) return null
  const record = raw as Record<string, unknown>
  const pinned = RecentAppScanSchema.safeParse({
    appId: record.appId,
    appName: record.appName,
    auditId: record.auditId,
    lastScannedAt: record.lastScannedAt,
  })
  if (!pinned.success) return null
  const entry: RecentAppEntry = { ...pinned.data }
  if (typeof record.generatedAt === 'string') entry.generatedAt = record.generatedAt
  if (record.stale === true) entry.stale = true
  return entry
}

export function loadRecentAppScans(): RecentAppEntry[] {
  return stored()
    .map(coerce)
    .filter((entry): entry is RecentAppEntry => entry !== null)
    .slice(0, RECENT_APP_MAX)
}

export function rememberRecentAppScan(input: {
  appId: string
  appName: string
  auditId: RecentAppScan['auditId']
  generatedAt?: string | null
}): void {
  const next: RecentAppEntry[] = [
    {
      appId: input.appId,
      appName: input.appName,
      auditId: input.auditId,
      lastScannedAt: new Date().toISOString(),
      ...(input.generatedAt ? { generatedAt: input.generatedAt } : {}),
    },
    ...loadRecentAppScans().filter((entry) => entry.appId !== input.appId),
  ].slice(0, RECENT_APP_MAX)
  window.localStorage.setItem(RECENT_APP_KEY, JSON.stringify(next))
}

/** A11: successful cleanup marks the session stale (sizes changed). */
export function markRecentAppStale(appId: string): void {
  const next = loadRecentAppScans().map((entry) => (entry.appId === appId ? { ...entry, stale: true } : entry))
  window.localStorage.setItem(RECENT_APP_KEY, JSON.stringify(next))
}

export function forgetRecentAppScan(appId: string): void {
  const next = loadRecentAppScans().filter((entry) => entry.appId !== appId)
  window.localStorage.setItem(RECENT_APP_KEY, JSON.stringify(next))
}

/**
 * A11: an entry is stale after cleanup or when its pinned generation no
 * longer matches the current report. Recents and rescans resolve against
 * the current report; a missing current id keeps the stored flag only.
 */
export function isRecentAppStale(entry: RecentAppEntry, currentAuditId?: string | null): boolean {
  if (entry.stale === true) return true
  if (currentAuditId && entry.auditId !== currentAuditId) return true
  return false
}
