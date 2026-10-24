// Unit tests for the advisory stale-process heuristic.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
import { STALE_THRESHOLDS, detectStaleProcesses, type StaleReason } from '../src/lib/memory/stale.ts'

const SAMPLED_AT = '2026-10-23T12:00:00.000Z'
const SAMPLED_MS = Date.parse(SAMPLED_AT)

function group(overrides: Partial<ProcessGroup> & { groupId: string }): ProcessGroup {
  return {
    name: 'worker',
    bundleId: null,
    bundlePath: null,
    kind: 'cli',
    mainPid: 100,
    pids: [100],
    cpuPercent: 0,
    footprintBytes: 1024,
    residentBytes: 512,
    frontmost: false,
    lastFrontmostAt: null,
    startedAt: new Date(SAMPLED_MS - 60_000).toISOString(),
    protection: null,
    ...overrides,
  }
}

function process(overrides: Partial<ProcessRow> & { pid: number; groupId: string; ageSeconds: number }): ProcessRow {
  const { ageSeconds, ...rest } = overrides
  return {
    ppid: 1,
    startTimeUs: (SAMPLED_MS - ageSeconds * 1_000) * 1_000,
    name: 'node',
    ownedByCurrentUser: true,
    kind: 'cli',
    cpuPercent: 0.1,
    footprintBytes: 1024,
    residentBytes: 512,
    virtualBytes: null,
    restricted: false,
    protection: null,
    command: 'node server.js',
    user: 'hk',
    state: 'sleeping',
    threads: 4,
    listeningPorts: [],
    ...rest,
  }
}

function snapshot(groups: ProcessGroup[], processes: ProcessRow[]): MemorySnapshot {
  return {
    sequence: 1,
    sampledAt: SAMPLED_AT,
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 100,
      usedBytes: 80,
      freeBytes: 20,
      availableBytes: 20,
      appBytes: null,
      wiredBytes: null,
      compressedBytes: null,
      cachedFilesBytes: null,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: 50 },
    groups,
    processes,
    notes: [],
  }
}

function reasonsOf(rows: ReturnType<typeof detectStaleProcesses>, pid: number): readonly StaleReason[] {
  return rows.find((row) => row.process.pid === pid)?.reasons ?? []
}

test('an orphaned runtime with a port is flagged with context', () => {
  const g = group({ groupId: 'grp_0000000000000001' })
  const p = process({
    pid: 101,
    groupId: g.groupId,
    ageSeconds: STALE_THRESHOLDS.runtimeMinAgeSeconds + 60,
    listeningPorts: [3080],
  })
  const rows = detectStaleProcesses(snapshot([g], [p]))
  assert.deepEqual(reasonsOf(rows, 101), ['runtimeLeftover', 'orphaned', 'port'])
})

test('system, bundled app, other-user and protected processes are never flagged', () => {
  const system = group({ groupId: 'grp_0000000000000002', kind: 'system', bundlePath: '/System/Library/x' })
  const app = group({ groupId: 'grp_0000000000000003', kind: 'app', bundlePath: '/Applications/X.app' })
  const own = group({ groupId: 'grp_0000000000000004' })
  const rows = detectStaleProcesses(
    snapshot(
      [system, app, own],
      [
        process({ pid: 1, groupId: system.groupId, ageSeconds: 10 * 60 * 60, kind: 'system' }),
        process({ pid: 2, groupId: app.groupId, ageSeconds: 10 * 60 * 60, kind: 'app' }),
        process({ pid: 3, groupId: own.groupId, ageSeconds: 10 * 60 * 60, ownedByCurrentUser: false }),
        process({ pid: 4, groupId: own.groupId, ageSeconds: 10 * 60 * 60, protection: 'otherUser' }),
      ],
    ),
  )
  assert.equal(rows.length, 0)
})

test('a young or busy orphan is not flagged', () => {
  const g = group({ groupId: 'grp_0000000000000005' })
  const young = process({ pid: 5, groupId: g.groupId, ageSeconds: 60 })
  const busy = process({ pid: 6, groupId: g.groupId, ageSeconds: 4 * 60 * 60, cpuPercent: 12 })
  const rows = detectStaleProcesses(snapshot([g], [young, busy]))
  assert.equal(rows.length, 0)
})

test('a non-runtime long idle process is not flagged without a port', () => {
  const g = group({ groupId: 'grp_0000000000000006' })
  const p = process({
    pid: 7,
    groupId: g.groupId,
    ageSeconds: STALE_THRESHOLDS.longIdleMinAgeSeconds + 60,
    name: 'rsync',
    command: '/usr/bin/rsync -a /src /dst',
  })
  assert.equal(detectStaleProcesses(snapshot([g], [p])).length, 0)
})

test('an orphaned non-runtime server with a port is flagged', () => {
  const g = group({ groupId: 'grp_0000000000000009' })
  const p = process({
    pid: 11,
    groupId: g.groupId,
    ageSeconds: STALE_THRESHOLDS.runtimeMinAgeSeconds + 60,
    name: 'server',
    command: '/opt/acme/server',
    listeningPorts: [8080],
  })
  assert.deepEqual(reasonsOf(detectStaleProcesses(snapshot([g], [p])), 11), ['orphaned', 'port'])
})

test('interactive shells are not reported just for being idle', () => {
  const g = group({ groupId: 'grp_0000000000000010' })
  const shell = process({
    pid: 12,
    groupId: g.groupId,
    ageSeconds: 5 * 60 * 60,
    name: 'zsh',
    command: '/bin/zsh -l',
  })
  assert.equal(detectStaleProcesses(snapshot([g], [shell])).length, 0)
})

test('zombie and stopped states are always flagged', () => {
  const g = group({ groupId: 'grp_0000000000000007' })
  const zombie = process({ pid: 8, groupId: g.groupId, ageSeconds: 5, state: 'zombie', cpuPercent: 50 })
  const stopped = process({ pid: 9, groupId: g.groupId, ageSeconds: 5, state: 'stopped', cpuPercent: null })
  const rows = detectStaleProcesses(snapshot([g], [zombie, stopped]))
  assert.deepEqual(reasonsOf(rows, 8), ['zombie'])
  assert.deepEqual(reasonsOf(rows, 9), ['stopped'])
})

test('a listening port alone does not make a process stale', () => {
  const g = group({ groupId: 'grp_0000000000000008' })
  const p = process({
    pid: 10,
    groupId: g.groupId,
    ageSeconds: 10 * 60,
    cpuPercent: 4,
    listeningPorts: [3000],
  })
  assert.equal(detectStaleProcesses(snapshot([g], [p])).length, 0)
})
