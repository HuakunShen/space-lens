import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { MemorySnapshot, ProcessRow } from '@space-lens/contract'
import { buildMemorySlice } from '../src/lib/memory/chart-model.ts'

function row(
  pid: number,
  ppid: number | null,
  bytes: number,
  name = 'node',
  command: string | null = null,
): ProcessRow {
  return {
    pid,
    ppid,
    startTimeUs: pid,
    name,
    ownedByCurrentUser: true,
    groupId: `g${pid}`,
    kind: 'cli',
    cpuPercent: 0,
    footprintBytes: bytes,
    residentBytes: bytes,
    virtualBytes: 0,
    restricted: false,
    protection: null,
    command,
    user: null,
    state: 'running',
    threads: null,
    listeningPorts: [],
  }
}
function snapshot(processes: ProcessRow[]): MemorySnapshot {
  return {
    processes,
    groups: processes.map((p) => ({
      groupId: p.groupId,
      name: p.name,
      bundleId: null,
      bundlePath: null,
      kind: p.kind,
      mainPid: p.pid,
      pids: [p.pid],
      cpuPercent: 0,
      footprintBytes: p.footprintBytes ?? 0,
      residentBytes: p.residentBytes ?? 0,
      frontmost: false,
      lastFrontmostAt: null,
      startedAt: '2026-01-01T00:00:00Z',
      protection: null,
    })),
  } as MemorySnapshot
}
test('focused slices follow parent links, account for self once, and limit depth', () => {
  const snap = snapshot([
    row(1, null, 10, 'app'),
    row(2, 1, 20, 'worker'),
    row(3, 2, 30, 'child'),
    row(4, 3, 40, 'leaf'),
  ])
  const one = buildMemorySlice(snap, 1, 1, false)
  assert.equal(one.total, 100)
  assert.equal(
    one.branches.reduce((n, b) => n + b.bytes, 0),
    100,
  )
  assert.deepEqual(one.branches.find((b) => b.name === 'worker')?.children, [])
  const three = buildMemorySlice(snap, 1, 3, false)
  const worker = three.branches.find((b) => b.name === 'worker')!
  assert.equal(worker.bytes, 90)
  assert.equal(
    worker.children.reduce((n, b) => n + b.bytes, 0),
    worker.bytes,
  )
  assert.equal(worker.children.find((b) => b.name === 'child')?.children.length, 2)
})
test('merges similar sibling services without merging unrelated node runtimes', () => {
  const snap = snapshot([
    row(1, null, 1, 'app'),
    row(2, 1, 10, 'node', 'node context7'),
    row(3, 1, 20, 'node', 'node context7'),
    row(4, 1, 5),
    row(5, 1, 6),
  ])
  const branches = buildMemorySlice(snap, 1, 2, true).branches
  const context = branches.find((b) => b.name === 'Context7')!
  assert.equal(context.bytes, 30)
  assert.deepEqual([...context.pids].sort(), [2, 3])
  assert.equal(branches.filter((b) => b.name.startsWith('node · PID')).length, 2)
})
test('root aggregates similar independent groups and keeps each navigation target', () => {
  const snap = snapshot([row(1, null, 10, 'node', 'node codegraph'), row(2, null, 20, 'node', 'node codegraph')])
  const branch = buildMemorySlice(snap, null, 2, true).branches[0]!
  assert.equal(branch.bytes, 30)
  assert.deepEqual(branch.pids, [1, 2])
})
test('large sibling lists fold smaller branches without losing their bytes', () => {
  const processes = [
    row(1, null, 5, 'app'),
    ...Array.from({ length: 40 }, (_, i) => row(i + 2, 1, 10 + i, `worker-${i}`)),
  ]
  const slice = buildMemorySlice(snapshot(processes), 1, 3, false)
  assert.ok(slice.branches.length <= 14)
  assert.equal(
    slice.branches.reduce((n, b) => n + b.bytes, 0),
    slice.total,
  )
  assert.ok(slice.branches.some((b) => b.kind === 'other'))
})

test('all-process sunburst includes bounded real hierarchy without counting other groups twice', () => {
  const app = row(1, null, 10, 'App')
  const child = row(2, 1, 20, 'renderer')
  const grandchild = row(3, 2, 30, 'worker')
  const external = row(4, 2, 40, 'external')
  child.groupId = app.groupId
  grandchild.groupId = app.groupId
  const snap = snapshot([app, child, grandchild, external])
  for (const group of snap.groups)
    if (group.groupId === app.groupId) {
      group.name = 'App'
      group.mainPid = 1
    }
  const root = buildMemorySlice(snap, null, 3, true)
  const appBranch = root.branches.find((branch) => branch.name === 'App')!
  assert.equal(root.total, 100)
  assert.equal(appBranch.bytes, 60)
  assert.equal(
    appBranch.children.reduce((n, branch) => n + branch.bytes, 0),
    60,
  )
  const renderer = appBranch.children.find((branch) => branch.name === 'renderer')!
  assert.equal(renderer.bytes, 50)
  assert.equal(
    renderer.children.reduce((n, branch) => n + branch.bytes, 0),
    50,
  )
  assert.deepEqual(renderer.children.find((branch) => branch.name === 'worker')?.pids, [3])
  assert.equal(
    buildMemorySlice(snap, null, 1, true).branches.find((branch) => branch.name === 'App')!.children.length,
    0,
  )
})

test('merged sibling branches retain descendant rings and account for their own memory', () => {
  const snap = snapshot([
    row(1, null, 5, 'app'),
    row(2, 1, 10, 'worker'),
    row(3, 1, 20, 'worker'),
    row(4, 2, 30, 'child-a'),
    row(5, 3, 40, 'child-b'),
  ])
  const merged = buildMemorySlice(snap, 1, 2, true).branches.find((branch) => branch.name === 'worker')!
  assert.equal(merged.bytes, 100)
  assert.equal(
    merged.children.reduce((n, branch) => n + branch.bytes, 0),
    100,
  )
  assert.deepEqual(
    merged.children
      .filter((branch) => branch.kind !== 'self')
      .map((branch) => branch.name)
      .sort(),
    ['child-a', 'child-b'],
  )
})
