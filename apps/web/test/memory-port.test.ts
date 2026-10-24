// Unit tests for the AI advisor's memory port over the workbench service
// (T22, §8 of docs/memory-cleaner-plan.md). The port is the only place the
// advisor touches the live snapshot and the client Quit list: reads map the
// contract's ProcessGroup/ProcessRow onto the advisor's `GroupView`, and
// `stageQuit` is the one append-only side effect, landing in the Quit list with
// the `advisor` origin.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type {
  CandidatesRequest,
  FreeUpCandidate,
  FreeUpCandidates,
  MemorySnapshot,
  ProcessGroup,
  ProcessRow,
} from '@space-lens/contract'
import type { WorkbenchService } from '@space-lens/client'
import { memoryPortFromService } from '../src/lib/memory/memory-port.ts'
import { QUIT_LIST_CAP, type QuitListEntry } from '../src/lib/memory/quit-list.ts'

function gid(n: number): string {
  return `grp_${n.toString(16).padStart(16, '0')}`
}

function group(id: string, overrides: Partial<ProcessGroup> = {}): ProcessGroup {
  return {
    groupId: id,
    name: id,
    bundleId: null,
    bundlePath: null,
    kind: 'app',
    mainPid: 100,
    pids: [100],
    cpuPercent: 1,
    footprintBytes: 1024,
    residentBytes: 512,
    frontmost: false,
    lastFrontmostAt: null,
    startedAt: '2026-10-18T00:00:00.000Z',
    protection: null,
    ...overrides,
  }
}

function row(pid: number, groupId: string, overrides: Partial<ProcessRow> = {}): ProcessRow {
  return {
    pid,
    ppid: null,
    startTimeUs: 1_000_000,
    name: `proc-${pid}`,
    ownedByCurrentUser: true,
    groupId,
    kind: 'app',
    cpuPercent: 1,
    footprintBytes: 1024,
    residentBytes: 512,
    virtualBytes: null,
    restricted: false,
    protection: null,
    command: null,
    user: null,
    state: 'sleeping',
    threads: null,
    listeningPorts: [],
    ...overrides,
  }
}

function snapshot(groups: ProcessGroup[], processes: ProcessRow[], overrides: Partial<MemorySnapshot> = {}): MemorySnapshot {
  return {
    sequence: 1,
    sampledAt: '2026-10-18T00:00:00.000Z',
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 10,
      usedBytes: 8,
      freeBytes: 2,
      availableBytes: 3,
      appBytes: 4,
      wiredBytes: 1,
      compressedBytes: 1,
      cachedFilesBytes: 1,
      swapTotalBytes: 4,
      swapUsedBytes: 1,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: 70 },
    groups,
    processes,
    notes: [],
    ...overrides,
  }
}

function candidate(id: string, overrides: Partial<FreeUpCandidate> = {}): FreeUpCandidate {
  return { groupId: id, name: id, footprintBytes: 1024, reasons: [], risks: [], preselected: false, ...overrides }
}

function candidateReply(overrides: Partial<FreeUpCandidates> = {}): FreeUpCandidates {
  return {
    sequence: 1,
    pressure: { level: 'warning', source: 'kernel', availablePercent: 40 },
    observedMinutes: 0,
    candidates: [],
    potentialReleaseBytes: 0,
    ...overrides,
  }
}

function fakeService(reply: FreeUpCandidates) {
  const calls: CandidatesRequest[] = []
  const service = {
    memoryCandidates: async (body: CandidatesRequest): Promise<FreeUpCandidates> => {
      calls.push(body)
      return reply
    },
  } as Pick<WorkbenchService, 'memoryCandidates'>
  return { service, calls }
}

function fakeQuitList(initial: QuitListEntry[] = []) {
  let list = initial
  return {
    handle: {
      entries: (): readonly QuitListEntry[] => list,
      replace: (next: QuitListEntry[]): void => {
        list = next
      },
    },
    current: () => list,
  }
}

test('the port walks the process tree up and down', async () => {
  const terminal = group(gid(1), { name: 'Terminal', mainPid: 100 })
  const host = group(gid(2), { name: 'spacelens serve', mainPid: 200 })
  const scan = group(gid(3), { name: 'kuntu-scan', mainPid: 300 })
  const { service } = fakeService(candidateReply())
  const port = memoryPortFromService(
    service,
    () =>
      snapshot(
        [terminal, host, scan],
        [
          row(100, terminal.groupId, { name: 'Terminal' }),
          row(200, host.groupId, { name: 'spacelens serve', ppid: 100 }),
          row(300, scan.groupId, { name: 'kuntu-scan', ppid: 200 }),
        ],
      ),
    fakeQuitList().handle,
  )

  const children = await port.children(terminal.groupId)
  assert.equal(children.total, 1)
  assert.deepEqual(children.rows.map((entry) => entry.name), ['spacelens serve'])
  assert.equal(children.rows[0].parentName, 'Terminal')

  const ancestry = await port.ancestry(scan.groupId)
  assert.deepEqual(ancestry?.chain.map((entry) => entry.name), ['Terminal', 'spacelens serve', 'kuntu-scan'])
  assert.equal(ancestry?.chain[1].parentName, 'Terminal')

  const unknown = await port.ancestry('grp_ffffffffffffffff')
  assert.equal(unknown, null)
})

test('summary reports pressure, breakdown, observed minutes and the largest groups', async () => {
  const alpha = group(gid(1), { name: 'Alpha', footprintBytes: 300 })
  const beta = group(gid(2), { name: 'Beta', footprintBytes: 500 })
  const { service } = fakeService(candidateReply({ observedMinutes: 12 }))
  const port = memoryPortFromService(service, () => snapshot([alpha, beta], [row(1, alpha.groupId), row(2, beta.groupId)]), fakeQuitList().handle)

  const summary = await port.summary()
  assert.equal(summary.observedMinutes, 12)
  assert.equal(summary.pressure.level, 'warning')
  assert.equal(summary.breakdown.totalBytes, 10)
  assert.deepEqual(summary.top.map((entry) => entry.name), ['Beta', 'Alpha'])
  assert.equal(summary.top[0].footprintBytes, 500)
  assert.equal(summary.top[0].processCount, 1)
})

test('summary falls back to the snapshot when candidates are unavailable', async () => {
  const only = group(gid(1), { footprintBytes: 400 })
  const service = {
    memoryCandidates: async (): Promise<FreeUpCandidates> => {
      throw new Error('read-only host')
    },
  } as Pick<WorkbenchService, 'memoryCandidates'>
  const port = memoryPortFromService(service, () => snapshot([only], []), fakeQuitList().handle)

  const summary = await port.summary()
  assert.equal(summary.observedMinutes, 0)
  assert.equal(summary.pressure.level, 'normal')
  assert.deepEqual(summary.top.map((entry) => entry.groupId), [only.groupId])
})

test('groups sorts by footprint and pages with the full total', async () => {
  const groups = [group(gid(1), { footprintBytes: 100 }), group(gid(2), { footprintBytes: 300 }), group(gid(3), { footprintBytes: 200 })]
  const port = memoryPortFromService(fakeService(candidateReply()).service, () => snapshot(groups, []), fakeQuitList().handle)

  const first = await port.groups('footprint', 2, 0)
  assert.equal(first.total, 3)
  assert.deepEqual(first.rows.map((entry) => entry.footprintBytes), [300, 200])
  const second = await port.groups('footprint', 2, 2)
  assert.deepEqual(second.rows.map((entry) => entry.footprintBytes), [100])
})

test('groups sorts by cpu with unreadable cpu last', async () => {
  const hot = group(gid(1), { cpuPercent: 90 })
  const cool = group(gid(2), { cpuPercent: 5 })
  const unknown = group(gid(3), { cpuPercent: null })
  const port = memoryPortFromService(fakeService(candidateReply()).service, () => snapshot([cool, unknown, hot], []), fakeQuitList().handle)

  const page = await port.groups('cpu', 10, 0)
  assert.deepEqual(page.rows.map((entry) => entry.groupId), [hot.groupId, cool.groupId, unknown.groupId])
})

test('group returns members, flags and the engine candidate, null when gone', async () => {
  const app = group(gid(1), { name: 'Dia', footprintBytes: 900, frontmost: true })
  const processes = [
    row(10, app.groupId, { name: 'Dia', footprintBytes: 800, protection: 'selfOrAncestor' }),
    row(11, app.groupId, { name: 'Dia Helper', footprintBytes: 100, restricted: true }),
  ]
  const reply = candidateReply({ candidates: [candidate(app.groupId, { preselected: true })] })
  const port = memoryPortFromService(fakeService(reply).service, () => snapshot([app], processes), fakeQuitList().handle)

  const detail = await port.group(app.groupId)
  assert.equal(detail?.group.name, 'Dia')
  assert.equal(detail?.group.frontmost, true)
  assert.equal(detail?.group.self, true)
  assert.equal(detail?.group.restricted, true)
  assert.deepEqual(detail?.members.map((member) => member.name), ['Dia', 'Dia Helper'])
  assert.equal(detail?.candidate?.preselected, true)
  assert.equal(await port.group(gid(9)), null)
})

test('stageQuit adds groups to the Quit list with the advisor origin', async () => {
  const alpha = group(gid(1), { name: 'Alpha' })
  const beta = group(gid(2), { name: 'Beta' })
  const quit = fakeQuitList()
  const port = memoryPortFromService(fakeService(candidateReply()).service, () => snapshot([alpha, beta], []), quit.handle)

  const outcome = await port.stageQuit([
    { groupId: alpha.groupId, reason: 'idle for 10 min' },
    { groupId: gid(9), reason: 'gone' },
  ])
  assert.deepEqual(outcome.staged, [alpha.groupId])
  assert.deepEqual(outcome.rejected, [{ groupId: gid(9), reason: 'no longer running' }])
  assert.equal(quit.current().length, 1)
  assert.equal(quit.current()[0].origin, 'advisor')
  assert.equal(quit.current()[0].reason, 'idle for 10 min')
  assert.equal(quit.current()[0].key, `group:${alpha.groupId}`)
})

test('stageQuit de-duplicates and caps the list through the shared reducer', async () => {
  const groups = Array.from({ length: 22 }, (_, index) => group(gid(index + 1)))
  const quit = fakeQuitList()
  const port = memoryPortFromService(fakeService(candidateReply()).service, () => snapshot(groups, []), quit.handle)

  const outcome = await port.stageQuit(groups.map((entry) => ({ groupId: entry.groupId, reason: 'x' })))
  assert.equal(outcome.staged.length, 22)
  assert.equal(quit.current().length, QUIT_LIST_CAP)
  assert.equal(quit.current()[0]?.groupId, groups[2]?.groupId)
})

test('candidate reads are cached per snapshot sequence', async () => {
  const { service, calls } = fakeService(candidateReply())
  const snap = snapshot([group(gid(1))], [])
  const port = memoryPortFromService(service, () => snap, fakeQuitList().handle)

  await port.summary()
  await port.group(gid(1))
  assert.equal(calls.length, 1)
})

test('groups never reads candidates', async () => {
  const { service, calls } = fakeService(candidateReply())
  const port = memoryPortFromService(service, () => snapshot([group(gid(1))], []), fakeQuitList().handle)

  await port.groups('footprint', 10, 0)
  assert.equal(calls.length, 0)
})
