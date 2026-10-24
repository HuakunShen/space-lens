/**
 * Sorting, filtering and order-freeze for the Memory process table (§6.2 and
 * the re-sort budget in §6.6 of docs/memory-cleaner-plan.md). Pure functions
 * only: they take a snapshot and return a new ordered array, never mutate the
 * snapshot, and never touch the transport.
 */
import type { MemorySnapshot, ProcessGroup } from '@space-lens/contract'

export type MemorySortKey = 'name' | 'cpu' | 'footprint' | 'resident' | 'virtual' | 'running' | 'pid'
export type MemoryFilter = 'all' | 'apps' | 'background' | 'mine' | 'protected' | 'ports'
export interface TableQuery {
  sort: MemorySortKey
  desc: boolean
  filter: MemoryFilter
  search: string
}

const EMPTY_VIRTUAL: ReadonlyMap<string, number> = new Map()

function numericValue(
  key: Exclude<MemorySortKey, 'name'>,
  group: ProcessGroup,
  virtualBytes: ReadonlyMap<string, number>,
): number | null {
  switch (key) {
    case 'cpu':
      return group.cpuPercent
    case 'footprint':
      return group.footprintBytes
    case 'resident':
      return group.residentBytes
    case 'virtual':
      return virtualBytes.get(group.groupId) ?? null
    case 'running': {
      // The column shows elapsed time, but sorting by the start timestamp is
      // the stable choice: elapsed time changes every tick, so a live sort
      // would make rows swap places while the pointer rests on them. Negated
      // so descending (the default for a numeric column) means "running
      // longest first", matching what the column shows.
      const startedAt = Date.parse(group.startedAt)
      return Number.isNaN(startedAt) ? null : -startedAt
    }
    case 'pid':
      return group.mainPid
  }
}

/** Group ids that own at least one process of the current user (the "Mine" filter). */
function ownedGroupIds(snap: MemorySnapshot): ReadonlySet<string> {
  const owned = new Set<string>()
  for (const process of snap.processes) {
    if (process.ownedByCurrentUser) owned.add(process.groupId)
  }
  return owned
}

/**
 * Summed virtual size per group. `ProcessGroup` carries no virtual field (the
 * column is hidden by default), so the table derives it from member processes;
 * a group with no readable member keeps `null` and sorts last.
 */
function groupVirtualBytes(snap: MemorySnapshot): ReadonlyMap<string, number> {
  const totals = new Map<string, number>()
  for (const process of snap.processes) {
    if (process.virtualBytes === null) continue
    totals.set(process.groupId, (totals.get(process.groupId) ?? 0) + process.virtualBytes)
  }
  return totals
}

function matchesFilter(
  group: ProcessGroup,
  filter: MemoryFilter,
  owned: ReadonlySet<string> | null,
  portGroups: ReadonlySet<string> | null,
): boolean {
  switch (filter) {
    case 'all':
      return true
    case 'apps':
      return group.kind === 'app'
    case 'background':
      return group.kind === 'agent'
    case 'mine':
      return owned !== null && owned.has(group.groupId)
    case 'protected':
      return group.protection !== null
    case 'ports':
      return portGroups !== null && portGroups.has(group.groupId)
  }
}

/** Group ids with at least one member listening on a TCP port. */
function groupsWithPorts(snap: MemorySnapshot): ReadonlySet<string> {
  const groups = new Set<string>()
  for (const process of snap.processes) {
    if (process.listeningPorts.length > 0) groups.add(process.groupId)
  }
  return groups
}

function matchesSearch(group: ProcessGroup, needle: string, details: ReadonlyMap<string, string> | null): boolean {
  if (needle === '') return true
  if (group.name.toLowerCase().includes(needle)) return true
  if (group.bundleId !== null && group.bundleId.toLowerCase().includes(needle)) return true
  if (group.bundlePath !== null && group.bundlePath.toLowerCase().includes(needle)) return true
  if (String(group.mainPid).includes(needle)) return true
  if (group.pids.some((pid) => String(pid).includes(needle))) return true
  return details?.get(group.groupId)?.includes(needle) ?? false
}

/**
 * One searchable string per group, built from its member processes. Searching
 * a command or an account is how people find "the node that is eating RAM";
 * the table used to stop at app name, bundle id and pid.
 */
function groupDetails(snap: MemorySnapshot): ReadonlyMap<string, string> {
  const details = new Map<string, string>()
  for (const process of snap.processes) {
    const fields = [process.command, process.user, process.state, process.listeningPorts.join(' ')].filter(
      (value): value is string => typeof value === 'string' && value !== '',
    )
    if (fields.length === 0) continue
    const previous = details.get(process.groupId)
    details.set(process.groupId, `${previous ?? ''} ${fields.join(' ')}`.toLowerCase())
  }
  return details
}

function tieBreak(a: ProcessGroup, b: ProcessGroup): number {
  return a.groupId < b.groupId ? -1 : a.groupId > b.groupId ? 1 : 0
}

/** Filter, search and sort the snapshot's groups for one table tick. */
export function selectGroups(snap: MemorySnapshot, q: TableQuery): ProcessGroup[] {
  const needle = q.search.trim().toLowerCase()
  const owned = q.filter === 'mine' ? ownedGroupIds(snap) : null
  const portGroups = q.filter === 'ports' ? groupsWithPorts(snap) : null
  const virtualBytes = q.sort === 'virtual' ? groupVirtualBytes(snap) : EMPTY_VIRTUAL
  const details = needle === '' ? null : groupDetails(snap)
  const rows = snap.groups.filter(
    (group) => matchesFilter(group, q.filter, owned, portGroups) && matchesSearch(group, needle, details),
  )
  const direction = q.desc ? -1 : 1
  rows.sort((a, b) => {
    if (q.sort === 'name') {
      const order = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
      return (order === 0 ? 0 : direction * order) || tieBreak(a, b)
    }
    const av = numericValue(q.sort, a, virtualBytes)
    const bv = numericValue(q.sort, b, virtualBytes)
    if (av === null || bv === null) {
      if (av === null && bv === null) return tieBreak(a, b)
      return av === null ? 1 : -1
    }
    if (av !== bv) return direction * (av - bv)
    return tieBreak(a, b)
  })
  return rows
}

/**
 * Re-sort jank guard (§6.6): while the pointer is over the table (or a row has
 * focus) the caller passes `frozen: true` and rows keep their previous order;
 * brand-new groups are appended and gone groups are dropped. `next` supplies
 * the live objects so values still update in place. When not frozen the current
 * tick's order (`next`) is returned as-is.
 */
export function applyOrderFreeze(
  prevOrder: readonly string[],
  next: readonly ProcessGroup[],
  frozen: boolean,
): ProcessGroup[] {
  if (!frozen) return [...next]
  const byId = new Map(next.map((group) => [group.groupId, group]))
  const ordered: ProcessGroup[] = []
  const seen = new Set<string>()
  for (const id of prevOrder) {
    const group = byId.get(id)
    if (group !== undefined) {
      ordered.push(group)
      seen.add(id)
    }
  }
  for (const group of next) {
    if (!seen.has(group.groupId)) ordered.push(group)
  }
  return ordered
}
