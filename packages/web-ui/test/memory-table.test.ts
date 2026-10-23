// Unit tests for the memory view's sort/filter/search and order freeze (T17).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { MemorySnapshot, ProcessGroup, ProcessRow } from '@space-lens/contract'
import { applyOrderFreeze, selectGroups, type MemoryFilter } from '../src/lib/memory/table.ts'

function group(overrides: Partial<ProcessGroup> & { groupId: string }): ProcessGroup {
  return {
    name: 'App',
    bundleId: null,
    bundlePath: null,
    kind: 'app',
    mainPid: 100,
    pids: [100],
    cpuPercent: null,
    footprintBytes: 0,
    residentBytes: 0,
    frontmost: false,
    lastFrontmostAt: null,
    startedAt: '2026-10-18T00:00:00.000Z',
    protection: null,
    ...overrides,
  }
}

function row(overrides: Partial<ProcessRow> & { pid: number; groupId: string }): ProcessRow {
  return {
    ppid: null,
    startTimeUs: overrides.pid,
    name: `proc${overrides.pid}`,
    ownedByCurrentUser: true,
    kind: 'app',
    cpuPercent: null,
    footprintBytes: null,
    residentBytes: null,
    virtualBytes: null,
    restricted: false,
    protection: null,
    ...overrides,
  }
}

function snapshot(groups: ProcessGroup[], processes: ProcessRow[] = []): MemorySnapshot {
  return {
    sequence: 1,
    sampledAt: '2026-10-18T00:00:00.000Z',
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 100,
      usedBytes: 100,
      freeBytes: 0,
      availableBytes: null,
      appBytes: null,
      wiredBytes: null,
      compressedBytes: null,
      cachedFilesBytes: null,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: null },
    groups,
    processes,
    notes: [],
  }
}

const alpha = group({ groupId: 'grp_0000000000000001', name: 'Alpha', mainPid: 11, footprintBytes: 300, cpuPercent: 5 })
const beta = group({ groupId: 'grp_0000000000000002', name: 'beta', mainPid: 22, footprintBytes: 100, cpuPercent: 20 })
const gamma = group({ groupId: 'grp_0000000000000003', name: 'Gamma', mainPid: 33, footprintBytes: 200, cpuPercent: null })

test('default style sort by footprint desc orders the largest first', () => {
  const out = selectGroups(snapshot([beta, gamma, alpha]), { sort: 'footprint', desc: true, filter: 'all', search: '' })
  assert.deepEqual(out.map((g) => g.name), ['Alpha', 'Gamma', 'beta'])
})

test('numeric sorts honour direction and keep missing values last', () => {
  const snap = snapshot([gamma, alpha, beta])
  assert.deepEqual(selectGroups(snap, { sort: 'footprint', desc: false, filter: 'all', search: '' }).map((g) => g.name), ['beta', 'Gamma', 'Alpha'])
  assert.deepEqual(selectGroups(snap, { sort: 'cpu', desc: true, filter: 'all', search: '' }).map((g) => g.name), ['beta', 'Alpha', 'Gamma'])
  assert.deepEqual(selectGroups(snap, { sort: 'cpu', desc: false, filter: 'all', search: '' }).map((g) => g.name), ['Alpha', 'beta', 'Gamma'])
  assert.deepEqual(selectGroups(snap, { sort: 'pid', desc: false, filter: 'all', search: '' }).map((g) => g.mainPid), [11, 22, 33])
})

test('name sort is case-insensitive', () => {
  const snap = snapshot([beta, gamma, alpha])
  assert.deepEqual(selectGroups(snap, { sort: 'name', desc: false, filter: 'all', search: '' }).map((g) => g.name), ['Alpha', 'beta', 'Gamma'])
  assert.deepEqual(selectGroups(snap, { sort: 'name', desc: true, filter: 'all', search: '' }).map((g) => g.name), ['Gamma', 'beta', 'Alpha'])
})

test('filters select apps, background agents, mine, and protected groups', () => {
  const mine = group({ groupId: 'grp_0000000000000010', name: 'Mine', kind: 'app' })
  const other = group({ groupId: 'grp_0000000000000011', name: 'Other', kind: 'app' })
  const agent = group({ groupId: 'grp_0000000000000012', name: 'Agent', kind: 'agent' })
  const protectedGroup = group({ groupId: 'grp_0000000000000013', name: 'Protected', kind: 'system', protection: 'systemComponent' })
  const snap = snapshot(
    [mine, other, agent, protectedGroup],
    [row({ pid: 1, groupId: mine.groupId }), row({ pid: 2, groupId: other.groupId, ownedByCurrentUser: false })],
  )
  const q = (filter: MemoryFilter) => selectGroups(snap, { sort: 'name', desc: false, filter, search: '' }).map((g) => g.name)
  assert.deepEqual(q('all'), ['Agent', 'Mine', 'Other', 'Protected'])
  assert.deepEqual(q('apps'), ['Mine', 'Other'])
  assert.deepEqual(q('background'), ['Agent'])
  assert.deepEqual(q('mine'), ['Mine'])
  assert.deepEqual(q('protected'), ['Protected'])
})

test('search matches name, bundle id and pid', () => {
  const named = group({ groupId: 'grp_0000000000000020', name: 'Dia', bundleId: 'com.dia.app', mainPid: 4242, pids: [4242] })
  const other = group({ groupId: 'grp_0000000000000021', name: 'Warp', bundleId: 'dev.warp.Warp-Stable', mainPid: 7, pids: [7, 8] })
  const snap = snapshot([named, other])
  const search = (s: string) => selectGroups(snap, { sort: 'name', desc: false, filter: 'all', search: s }).map((g) => g.name)
  assert.deepEqual(search('dia'), ['Dia'])
  assert.deepEqual(search('COM.DIA'), ['Dia'])
  assert.deepEqual(search('warp-stable'), ['Warp'])
  assert.deepEqual(search('4242'), ['Dia'])
  assert.deepEqual(search('8'), ['Warp'])
  assert.deepEqual(search('   '), ['Dia', 'Warp'])
  assert.deepEqual(search('nothing'), [])
})

test('applyOrderFreeze keeps previous order, appends new groups and drops gone ones', () => {
  const a = group({ groupId: 'grp_000000000000000a', name: 'A', footprintBytes: 1 })
  const b = group({ groupId: 'grp_000000000000000b', name: 'B', footprintBytes: 2 })
  const c = group({ groupId: 'grp_000000000000000c', name: 'C', footprintBytes: 3 })
  const next = [a, c, b]
  assert.deepEqual(applyOrderFreeze([b.groupId, a.groupId], next, true).map((g) => g.name), ['B', 'A', 'C'])
  assert.deepEqual(applyOrderFreeze([b.groupId, a.groupId], [a], true).map((g) => g.name), ['A'])
  assert.deepEqual(applyOrderFreeze([b.groupId, a.groupId], next, false).map((g) => g.name), ['A', 'C', 'B'])
  const frozen = applyOrderFreeze([b.groupId, a.groupId], next, true)
  assert.equal(frozen[0], b, 'frozen rows are the live objects from the new tick')
})

test('virtual sort sums member process sizes and keeps unreadable groups last', () => {
  const a = group({ groupId: 'grp_0000000000000030', name: 'A', mainPid: 1 })
  const b = group({ groupId: 'grp_0000000000000031', name: 'B', mainPid: 2 })
  const snap = snapshot(
    [b, a],
    [
      row({ pid: 1, groupId: a.groupId, virtualBytes: 300 }),
      row({ pid: 3, groupId: a.groupId, virtualBytes: 200 }),
      row({ pid: 2, groupId: b.groupId, virtualBytes: null }),
    ],
  )
  const order = (desc: boolean) => selectGroups(snap, { sort: 'virtual', desc, filter: 'all', search: '' }).map((g) => g.name)
  assert.deepEqual(order(true), ['A', 'B'])
  assert.deepEqual(order(false), ['A', 'B'])
})

test('selecting 1000 processes across 300 groups stays under the 20 ms budget', () => {
  const groups: ProcessGroup[] = []
  const processes: ProcessRow[] = []
  for (let i = 0; i < 300; i += 1) {
    const id = `grp_${i.toString(16).padStart(16, '0')}`
    groups.push(group({ groupId: id, name: `Group ${i}`, mainPid: 1000 + i, footprintBytes: i * 1024, cpuPercent: (i % 20) + 0.5 }))
  }
  for (let i = 0; i < 1000; i += 1) {
    processes.push(row({ pid: 2000 + i, groupId: groups[i % groups.length]!.groupId }))
  }
  const snap = snapshot(groups, processes)
  const started = performance.now()
  const out = selectGroups(snap, { sort: 'footprint', desc: true, filter: 'all', search: 'group 1' })
  const elapsed = performance.now() - started
  assert.ok(out.length > 0)
  assert.ok(elapsed < 20, `selectGroups took ${elapsed.toFixed(2)} ms, budget is 20 ms`)
})
