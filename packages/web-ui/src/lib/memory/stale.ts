/**
 * Advisory detection of processes that look left behind.
 *
 * This is deliberately a heuristic, not a verdict: Space Lens never quits
 * anything from here. A process is a suspect when it is owned by the current
 * user, unprotected, not a bundled app/system component, and at least one of
 * these holds:
 *
 * - it is a zombie or stopped process;
 * - its parent is gone (launchd, or a pid no longer in the snapshot) and it has
 *   been sitting at low CPU for long enough;
 * - it is a known runtime (node, bun, python, java, …) orphaned for a while —
 *   the "IDE/AI spawned it and exited" case — or idle for hours;
 * - it is an orphaned process listening on a port.
 *
 * Generic long-idle non-runtimes are deliberately excluded: a dozen idle zsh
 * sessions or a menu-bar broker are not leftovers.
 *
 * A listening port is shown as context, and promotes an already-suspicious
 * process with the `port` reason, but a healthy dev server is never flagged
 * just for holding a port. Interactive shells are excluded: they are terminal
 * sessions, not leftovers, and a wall of zsh rows is noise.
 */
import type { MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
import { runningSeconds } from './format.ts'

export const STALE_REASONS = ['zombie', 'stopped', 'runtimeLeftover', 'orphaned', 'longIdle', 'port'] as const
export type StaleReason = (typeof STALE_REASONS)[number]

export const STALE_REASON_LABELS: Record<StaleReason, string> = {
  zombie: 'Zombie process',
  stopped: 'Stopped or suspended',
  runtimeLeftover: 'Runtime left behind',
  orphaned: 'Parent exited',
  longIdle: 'Long idle',
  port: 'Listening on a port',
}

export const STALE_THRESHOLDS = {
  /** An orphan must be this old before it counts. */
  orphanMinAgeSeconds: 10 * 60,
  /** A known runtime gets flagged a little later than a plain orphan. */
  runtimeMinAgeSeconds: 30 * 60,
  /** Long-idle is the slow path: hours, not minutes. */
  longIdleMinAgeSeconds: 2 * 60 * 60,
  /** Below this CPU share an orphan is considered idle. */
  lowCpuPercent: 1,
  /** Below this share, a process has been idle for a long time. */
  idleCpuPercent: 0.5,
} as const

/** Runtimes commonly spawned by editors, AI tools and package scripts. */
const RUNTIME_COMMAND =
  /(?:^|[\s/])(node|nodejs|bun|deno|python[0-9.]*|ruby|java|php|go|cargo|rustc|dotnet|npm|pnpm|yarn|npx|vite|webpack|esbuild|tsx|ts-node|opencode|claude|code)(?:\s|$)/i

/** Interactive shells are terminal sessions, not leftovers. */
const SHELL_COMMAND = /(?:^|[\s/])(zsh|bash|sh|fish|tcsh|csh|login)(?:\s|$)/

export interface StaleProcess {
  readonly process: ProcessRow
  readonly group: ProcessGroup | null
  readonly reasons: readonly StaleReason[]
  /** Elapsed seconds at the snapshot's sample time; 0 when unknown. */
  readonly ageSeconds: number
  /** Higher first; reasons weigh more than age. */
  readonly score: number
}

const REASON_WEIGHT: Record<StaleReason, number> = {
  zombie: 500,
  stopped: 400,
  runtimeLeftover: 300,
  orphaned: 200,
  longIdle: 150,
  port: 50,
}

/** Whether a process kind can plausibly be a leftover background runtime. */
function isReviewableKind(process: ProcessRow, group: ProcessGroup | null): boolean {
  if (process.kind === 'system' || process.kind === 'app' || process.kind === 'helper') return false
  if (group?.kind === 'app' || group?.kind === 'system') return false
  if (group?.bundlePath !== null && group?.bundlePath !== undefined) return false
  return true
}

function isOrphan(process: ProcessRow, pids: ReadonlySet<number>): boolean {
  if (process.ppid === null) return true
  if (process.ppid === 1) return true
  return !pids.has(process.ppid)
}

function isRuntime(process: ProcessRow): boolean {
  return process.command !== null && RUNTIME_COMMAND.test(process.command)
}

function isShell(process: ProcessRow): boolean {
  return process.command !== null && SHELL_COMMAND.test(process.command)
}

/** Detect the advisory stale-process list for one snapshot, highest score first. */
export function detectStaleProcesses(snapshot: MemorySnapshot): StaleProcess[] {
  const pids = new Set(snapshot.processes.map((process) => process.pid))
  const groups = new Map(snapshot.groups.map((group) => [group.groupId, group]))
  const out: StaleProcess[] = []

  for (const process of snapshot.processes) {
    if (!process.ownedByCurrentUser || process.protection !== null) continue
    const group = groups.get(process.groupId) ?? null
    if (!isReviewableKind(process, group)) continue

    const ageSeconds = runningSeconds(process.startTimeUs, snapshot.sampledAt) ?? 0
    const cpu = process.cpuPercent
    const lowCpu = cpu !== null && cpu < STALE_THRESHOLDS.lowCpuPercent
    const idleCpu = cpu !== null && cpu < STALE_THRESHOLDS.idleCpuPercent
    const orphan = isOrphan(process, pids)
    const reasons: StaleReason[] = []

    if (process.state === 'zombie') reasons.push('zombie')
    if (process.state === 'stopped') reasons.push('stopped')

    // Zombie/stopped is high confidence even for a shell. Every other rule
    // skips interactive shells: a terminal that outlived its window is normal.
    if (reasons.length === 0 && isShell(process)) continue

    const port = process.listeningPorts.length > 0
    const runtime = isRuntime(process)
    const longIdle = ageSeconds >= STALE_THRESHOLDS.longIdleMinAgeSeconds && idleCpu

    if (
      reasons.length === 0 &&
      runtime &&
      lowCpu &&
      ((orphan && ageSeconds >= STALE_THRESHOLDS.runtimeMinAgeSeconds) || longIdle)
    ) {
      reasons.push('runtimeLeftover')
      if (orphan) reasons.push('orphaned')
      if (longIdle) reasons.push('longIdle')
    } else if (
      reasons.length === 0 &&
      orphan &&
      port &&
      lowCpu &&
      ageSeconds >= STALE_THRESHOLDS.runtimeMinAgeSeconds
    ) {
      // A non-runtime server that outlived its parent: the port is the only
      // reason we can see, so it is shown as evidence rather than proof.
      reasons.push('orphaned')
    }

    if (port && reasons.length > 0) reasons.push('port')

    if (reasons.length === 0) continue
    const score = reasons.reduce((total, reason) => total + REASON_WEIGHT[reason], 0) + ageSeconds / 60
    out.push({ process, group, reasons, ageSeconds, score })
  }

  out.sort((a, b) => b.score - a.score || a.process.pid - b.process.pid)
  return out
}
