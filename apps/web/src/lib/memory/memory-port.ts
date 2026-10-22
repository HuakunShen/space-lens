/**
 * AI advisor memory port for the Memory view (T22, §8 of
 * docs/memory-cleaner-plan.md).
 *
 * Maps the workbench service and the page's live snapshot onto the advisor's
 * `MemoryPort`, so the memory tools see `GroupView` rows with no pids, start
 * times, paths, bundle ids or command lines. The port is read-mostly: `summary`,
 * `groups` and `group` read the latest snapshot (plus the engine's free-up
 * verdict, cached per snapshot sequence), and `stageQuit` is the one
 * append-only side effect — it adds ids to the client Quit list the user
 * reviews, never plans or quits anything.
 */
import type { FreeUpCandidates, MemorySnapshot, ProcessGroup } from '@space-lens/contract'
import type { WorkbenchService } from '@space-lens/client'
import type {
  GroupAncestry,
  GroupDetail,
  GroupMember,
  GroupPage,
  GroupView,
  MemoryPort,
  MemorySummaryView,
  QuitStageItem,
  QuitStageOutcome,
} from '@space-lens/ai-advisor'
import { buildGroupTree, groupAncestry, type GroupTree } from '@space-lens/web-ui/lib/memory/tree'
import { addToQuitList, type QuitListEntry } from './quit-list.ts'

/** The service methods the port reads; a host with memory sampling serves them. */
export type MemoryPortService = Pick<WorkbenchService, 'memoryCandidates'>

/** The page's Quit list, read and replaced so a stage lands in the live UI. */
export interface QuitListHandle {
  /** The current entries, oldest first. */
  entries(): readonly QuitListEntry[]
  /** Replaces the list; the page re-assigns its `$state`. */
  replace(entries: QuitListEntry[]): void
}

/** Rows in the summary header window (matching `get_memory_summary`). */
const TOP_LIMIT = 10

export function memoryPortFromService(
  service: MemoryPortService,
  getSnapshot: () => MemorySnapshot | null,
  quitList: QuitListHandle,
): MemoryPort {
  // The engine's verdict for one snapshot sequence; re-read when the sequence
  // advances, so `summary` and `group` never ask the host twice per tick.
  let cached: { sequence: number; value: FreeUpCandidates } | null = null
  // The group tree for one snapshot sequence: which group launched which.
  let cachedTree: { sequence: number; value: GroupTree; names: Map<string, string> } | null = null

  const latest = (): MemorySnapshot | null => getSnapshot()

  function requireSnapshot(): MemorySnapshot {
    const snapshot = latest()
    if (snapshot === null) throw new Error('the memory view has no snapshot yet')
    return snapshot
  }

  /** The group tree of the current snapshot, rebuilt only when it changes. */
  function tree(): { value: GroupTree; names: Map<string, string> } {
    const snapshot = requireSnapshot()
    if (cachedTree !== null && cachedTree.sequence === snapshot.sequence) {
      return { value: cachedTree.value, names: cachedTree.names }
    }
    const names = new Map<string, string>()
    for (const group of snapshot.groups) names.set(group.groupId, group.name)
    const value = buildGroupTree(snapshot)
    cachedTree = { sequence: snapshot.sequence, value, names }
    return { value, names }
  }

  async function candidates(): Promise<FreeUpCandidates | null> {
    const snapshot = latest()
    if (snapshot === null) return null
    const read = service.memoryCandidates
    if (read === undefined) return null
    if (cached !== null && cached.sequence === snapshot.sequence) return cached.value
    try {
      const value = await read.call(service, { exclude: [] })
      cached = { sequence: snapshot.sequence, value }
      return value
    } catch {
      // Candidates are best-effort: a read-only or failing host still lists groups.
      return null
    }
  }

  function viewOf(snapshot: MemorySnapshot, group: ProcessGroup): GroupView {
    const rows = snapshot.processes.filter((row) => row.groupId === group.groupId)
    const { value, names } = tree()
    const parentId = value.parentOf.get(group.groupId) ?? null
    return {
      groupId: group.groupId,
      name: group.name,
      kind: group.kind,
      footprintBytes: group.footprintBytes,
      cpuPercent: group.cpuPercent,
      processCount: group.pids.length,
      frontmost: group.frontmost,
      protected: group.protection !== null || rows.some((row) => row.protection !== null),
      // `self` covers the host and everything it spawned (the stdio bridge, the
      // scan CLI): staging either would break the view or the host.
      self: rows.some((row) => row.protection === 'selfOrAncestor' || row.protection === 'selfChild'),
      restricted: rows.some((row) => row.restricted),
      parentName: parentId === null ? null : (names.get(parentId) ?? null),
    }
  }

  function membersOf(snapshot: MemorySnapshot, groupId: string): GroupMember[] {
    return snapshot.processes
      .filter((row) => row.groupId === groupId)
      .map((row) => ({ name: row.name, footprintBytes: row.footprintBytes, cpuPercent: row.cpuPercent }))
  }

  function ordered(groups: readonly ProcessGroup[], sort: 'footprint' | 'cpu'): ProcessGroup[] {
    const sorted = [...groups]
    if (sort === 'cpu') sorted.sort((a, b) => (b.cpuPercent ?? -1) - (a.cpuPercent ?? -1))
    else sorted.sort((a, b) => b.footprintBytes - a.footprintBytes)
    return sorted
  }

  return {
    async summary(): Promise<MemorySummaryView> {
      const snapshot = requireSnapshot()
      const fresh = await candidates()
      return {
        pressure: fresh?.pressure ?? snapshot.pressure,
        breakdown: snapshot.breakdown,
        observedMinutes: fresh?.observedMinutes ?? 0,
        top: ordered(snapshot.groups, 'footprint').slice(0, TOP_LIMIT).map((group) => viewOf(snapshot, group)),
      }
    },

    async groups(sort, limit, offset): Promise<GroupPage> {
      const snapshot = requireSnapshot()
      const sorted = ordered(snapshot.groups, sort)
      return {
        rows: sorted.slice(offset, offset + limit).map((group) => viewOf(snapshot, group)),
        total: sorted.length,
      }
    },

    async group(groupId: string): Promise<GroupDetail | null> {
      const snapshot = latest()
      if (snapshot === null) return null
      const group = snapshot.groups.find((entry) => entry.groupId === groupId)
      if (group === undefined) return null
      const fresh = await candidates()
      return {
        group: viewOf(snapshot, group),
        members: membersOf(snapshot, groupId),
        candidate: fresh?.candidates.find((entry) => entry.groupId === groupId) ?? null,
      }
    },

    async children(groupId: string): Promise<GroupPage> {
      const snapshot = latest()
      if (snapshot === null) return { rows: [], total: 0 }
      const { value } = tree()
      const ids = value.childrenOf.get(groupId) ?? []
      const rows: GroupView[] = []
      for (const id of ids) {
        const group = snapshot.groups.find((entry) => entry.groupId === id)
        if (group !== undefined) rows.push(viewOf(snapshot, group))
      }
      return { rows, total: rows.length }
    },

    async ancestry(groupId: string): Promise<GroupAncestry | null> {
      const snapshot = latest()
      if (snapshot === null) return null
      const { value } = tree()
      const chain: GroupView[] = []
      for (const id of groupAncestry(value, groupId)) {
        const group = snapshot.groups.find((entry) => entry.groupId === id)
        if (group !== undefined) chain.push(viewOf(snapshot, group))
      }
      if (chain.length === 0) return null
      return { chain }
    },

    async stageQuit(items: QuitStageItem[]): Promise<QuitStageOutcome> {
      const snapshot = latest()
      const staged: string[] = []
      const rejected: { groupId: string; reason: string }[] = []
      const stagedAt = new Date().toISOString()
      let list: QuitListEntry[] = [...quitList.entries()]
      for (const item of items) {
        const group = snapshot?.groups.find((entry) => entry.groupId === item.groupId)
        if (group === undefined) {
          rejected.push({ groupId: item.groupId, reason: 'no longer running' })
          continue
        }
        list = addToQuitList(list, {
          key: `group:${group.groupId}`,
          groupId: group.groupId,
          name: group.name,
          origin: 'advisor',
          reason: item.reason,
          stagedAt,
        })
        staged.push(group.groupId)
      }
      quitList.replace(list)
      return { staged, rejected }
    },
  }
}
