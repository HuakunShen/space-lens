/**
 * Software presets: a named group of known software that the storage and the
 * memory views can total together ("AI tools", later "Browsers", ...).
 *
 * One rule describes one program. It lists the app-audit catalog ids that hold
 * its files (storage) and how to recognise its live processes (memory). Adding
 * a program is a data change here, not new view code. See `CONTRIBUTING.md`.
 */
import type { AppAuditEntry, AppAuditReport, MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
import { buildForest, type ProcessForest, type TreeNode } from '../memory/tree.ts'

export interface SoftwareRule {
  /** Stable id, used as the selection key. Unique within its preset. */
  readonly id: string
  /** Name shown in the UI. */
  readonly name: string
  /** Ids in `crates/app-audit/catalogs/macos.json` whose storage counts for this rule. */
  readonly storage: readonly string[]
  /**
   * How a live process is recognised. A process matches when any `names` entry
   * equals its name (case-insensitive) or any `commands` pattern matches its
   * full command line. Both are optional, but a rule needs at least one.
   */
  readonly match: {
    readonly names?: readonly string[]
    readonly commands?: readonly RegExp[]
  }
}

export interface SoftwarePreset {
  readonly id: string
  readonly label: string
  readonly description: string
  readonly rules: readonly SoftwareRule[]
}

/** Does this process belong to the rule? */
export function ruleMatchesProcess(rule: SoftwareRule, row: Pick<ProcessRow, 'name' | 'command'>): boolean {
  const name = row.name.toLowerCase()
  if (rule.match.names?.some((candidate) => candidate.toLowerCase() === name)) return true
  const command = row.command ?? ''
  return rule.match.commands?.some((pattern) => pattern.test(command)) ?? false
}

/** Pids of a node and everything below it. */
function subtreePids(node: TreeNode, into: Set<number> = new Set()): Set<number> {
  into.add(node.pid)
  for (const child of node.children) subtreePids(child, into)
  return into
}

function hasAncestor(forest: ProcessForest, node: TreeNode, pids: ReadonlySet<number>): boolean {
  let parent = node.ppid
  const seen = new Set<number>()
  while (parent !== null && !seen.has(parent)) {
    if (pids.has(parent)) return true
    seen.add(parent)
    parent = forest.byPid.get(parent)?.ppid ?? null
  }
  return false
}

/** One matched process and the rule that claimed it (first rule in the preset wins). */
interface Claim {
  readonly node: TreeNode
  readonly rule: SoftwareRule
}

function claimProcesses(snapshot: MemorySnapshot, preset: SoftwarePreset, forest: ProcessForest): Claim[] {
  const claims: Claim[] = []
  for (const row of snapshot.processes) {
    const node = forest.byPid.get(row.pid)
    if (node === undefined) continue
    const rule = preset.rules.find((candidate) => ruleMatchesProcess(candidate, row))
    if (rule !== undefined) claims.push({ node, rule })
  }
  return claims
}

export interface PresetMemoryRow {
  readonly rule: SoftwareRule
  /** Memory of this rule's own top-level processes, including their children. */
  readonly bytes: number
  /** Every process in those subtrees. */
  readonly processCount: number
  /** Pids of those subtrees, for scoping the views. */
  readonly pids: ReadonlySet<number>
}

export interface PresetMemorySummary {
  /** Memory of every matched process, each subtree counted once. */
  readonly totalBytes: number
  readonly processCount: number
  readonly rows: readonly PresetMemoryRow[]
}

/**
 * Totals a preset over one snapshot. A matched process nested under another
 * matched process (Codex under ChatGPT, say) is counted once, inside its
 * outermost match. Per-rule rows keep their own nested processes, so a row can
 * overlap a neighbour's row; `totalBytes` never double-counts.
 */
export function summarizePresetMemory(snapshot: MemorySnapshot, preset: SoftwarePreset): PresetMemorySummary {
  const forest = buildForest(snapshot)
  const claims = claimProcesses(snapshot, preset, forest)
  const matched = new Set(claims.map((claim) => claim.node.pid))

  let totalBytes = 0
  const outerPids = new Set<number>()
  for (const claim of claims) {
    if (hasAncestor(forest, claim.node, matched)) continue
    totalBytes += claim.node.subtreeBytes
    for (const pid of subtreePids(claim.node)) outerPids.add(pid)
  }

  const rows: PresetMemoryRow[] = preset.rules.map((rule) => {
    const owned = claims.filter((claim) => claim.rule === rule).map((claim) => claim.node)
    const ruleMatched = new Set(owned.map((node) => node.pid))
    const roots = owned.filter((node) => !hasAncestor(forest, node, ruleMatched))
    const pids = new Set<number>()
    let bytes = 0
    for (const root of roots) {
      bytes += root.subtreeBytes
      for (const pid of subtreePids(root)) pids.add(pid)
    }
    return { rule, bytes, processCount: pids.size, pids }
  })

  return { totalBytes, processCount: outerPids.size, rows }
}

/**
 * Keep only the processes of one rule (or of every rule when `ruleId` is null)
 * and their subtrees. Groups are narrowed to the kept processes so the tables
 * and the chart total what is on screen.
 */
export function scopeSnapshot(snapshot: MemorySnapshot, preset: SoftwarePreset, ruleId: string | null): MemorySnapshot {
  const forest = buildForest(snapshot)
  const claims = claimProcesses(snapshot, preset, forest)
  const chosen = claims.filter((claim) => ruleId === null || claim.rule.id === ruleId)
  const chosenPids = new Set(chosen.map((claim) => claim.node.pid))
  const keep = new Set<number>()
  for (const claim of chosen) {
    if (hasAncestor(forest, claim.node, chosenPids)) continue
    for (const pid of subtreePids(claim.node)) keep.add(pid)
  }

  const processes = snapshot.processes.filter((row) => keep.has(row.pid))
  const groups: ProcessGroup[] = []
  for (const group of snapshot.groups) {
    const pids = group.pids.filter((pid) => keep.has(pid))
    if (pids.length === 0) continue
    const members = processes.filter((row) => row.groupId === group.groupId)
    groups.push({
      ...group,
      pids,
      mainPid: pids.includes(group.mainPid) ? group.mainPid : pids[0]!,
      footprintBytes: members.reduce((sum, row) => sum + (row.footprintBytes ?? 0), 0),
      residentBytes: members.reduce((sum, row) => sum + (row.residentBytes ?? 0), 0),
    })
  }
  return { ...snapshot, processes, groups }
}

export interface PresetStorageRow {
  readonly rule: SoftwareRule
  readonly bytes: number
  readonly entries: readonly AppAuditEntry[]
}

export interface PresetStorageSummary {
  readonly totalBytes: number
  readonly rows: readonly PresetStorageRow[]
}

/** Totals a preset over one app-audit report, using each catalog entry's total size. */
export function summarizePresetStorage(report: AppAuditReport | null, preset: SoftwarePreset): PresetStorageSummary {
  const byId = new Map((report?.apps ?? []).map((app) => [app.id, app]))
  const seen = new Set<string>()
  const rows = preset.rules.map((rule) => {
    const entries: AppAuditEntry[] = []
    for (const id of rule.storage) {
      const entry = byId.get(id)
      if (entry === undefined || seen.has(id)) continue
      seen.add(id)
      entries.push(entry)
    }
    return { rule, bytes: entries.reduce((sum, entry) => sum + entry.totalSize, 0), entries }
  })
  return { totalBytes: rows.reduce((sum, row) => sum + row.bytes, 0), rows }
}
