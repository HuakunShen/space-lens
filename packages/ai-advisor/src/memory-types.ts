import type { FreeUpCandidate, MemoryBreakdown, MemoryPressure, ProcessGroup } from '@space-lens/contract'

/**
 * Memory-view port for the AI advisor (§8 of docs/memory-cleaner-plan.md).
 *
 * The advisor never touches the memory service directly: hosts map their own
 * snapshot/plan surface onto `MemoryPort`, exactly as they map disk reads onto
 * `LensPort`. The port is read-only plus one append-only side effect,
 * `stageQuit`, which adds groups to the client-side Quit list the user reviews.
 * There is no plan or execute method here, and no `mode` anywhere: the model
 * cannot ask for force, and it cannot quit anything itself.
 *
 * What the model is allowed to see is decided by this package, not by the host:
 * `GroupView` carries no pids, start times, paths, bundle ids or command lines.
 */

/** One group kind, written once by the contract and derived here (matching `web-ui`'s `PressureLevel`). */
export type ProcessKind = ProcessGroup['kind']

/** One app/process group as the advisor sees it; no ids beyond the opaque group id. */
export interface GroupView {
  /** Opaque engine id (`grp_…`); the only handle the model gets. */
  groupId: string
  name: string
  kind: ProcessKind
  /** Activity-Monitor-style footprint, or RSS on platforms without it. */
  footprintBytes: number
  cpuPercent: number | null
  processCount: number
  frontmost: boolean
  /** Any member is protected by a §5 rule. */
  protected: boolean
  /** The group holds the advisor's host process or one of its ancestors. */
  self: boolean
  /** Footprint is unreadable (other user / system); the row shows RSS only. */
  restricted: boolean
  /**
   * The name of the group that launched this one, or `null` when it has no
   * known parent. A name, never an id: the id of the parent is what the tree
   * tools hand out.
   */
  parentName?: string | null
}

/** The one-call overview placed at the head of a memory-only run. */
export interface MemorySummaryView {
  pressure: MemoryPressure
  breakdown: MemoryBreakdown
  /** Minutes the memory view has been open and sampling. */
  observedMinutes: number
  /** Largest groups first, at most 10. */
  top: GroupView[]
}

/** One page of the group table. */
export interface GroupPage {
  rows: GroupView[]
  total: number
}

/** One member process inside a group; names only, never pids or paths. */
export interface GroupMember {
  name: string
  footprintBytes: number | null
  cpuPercent: number | null
}

/** One group with its members and the engine's free-up verdict, if any. */
export interface GroupDetail {
  group: GroupView
  members: GroupMember[]
  candidate: FreeUpCandidate | null
}

/**
 * The chain of groups that led to one group: root first, the group itself
 * last. This is the process tree the disk advisor walks with `list_children`,
 * seen from a leaf: "what launched this?".
 */
export interface GroupAncestry {
  chain: GroupView[]
}

/** One requested Quit-list addition, as the model asked for it. */
export interface QuitStageItem {
  groupId: string
  reason: string
}

/** What the host's Quit list accepted, and why it refused the rest. */
export interface QuitStageOutcome {
  staged: string[]
  rejected: { groupId: string; reason: string }[]
}

/**
 * The read-only window onto memory and the one append-only stage call.
 * Implemented by `memoryPortFromService` (T22) over a real service, and by
 * fakes in tests, so the tools never need a running sampler.
 */
export interface MemoryPort {
  /** Pressure, system totals and the top groups (one bounded call). */
  summary(): Promise<MemorySummaryView>
  /** One page of groups, largest first (or busiest first for `'cpu'`). */
  groups(sort: 'footprint' | 'cpu', limit: number, offset: number): Promise<GroupPage>
  /** One group with its members and the engine's candidate verdict; `null` when it is gone. */
  group(groupId: string): Promise<GroupDetail | null>
  /**
   * The groups this group launched, largest first — the process tree the disk
   * advisor walks with `list_children`, one level up from `groups`.
   */
  children(groupId: string): Promise<GroupPage>
  /** The chain that led to this group, root first; `null` when it is gone. */
  ancestry(groupId: string): Promise<GroupAncestry | null>
  /**
   * Adds ids to the client Quit list the user reviews; never plans or quits.
   * The host owns that list, including its 20-entry cap (§8 rule 2, T20's
   * `addToQuitList`), and reports what it refused in the outcome.
   */
  stageQuit(items: QuitStageItem[]): Promise<QuitStageOutcome>
}
