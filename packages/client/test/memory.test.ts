/**
 * Memory view and process-control client tests (T15, §4.5/§4.7/§4.8 of
 * docs/memory-cleaner-plan.md). The HTTP adapter must hit the frozen routes
 * with the exact bodies and re-validate every reply against the contract
 * schemas; the Tauri adapter must send the exact closed `sl_read` / `sl_submit`
 * / `sl_host_request` payloads (with the default 60 s reply timeout for
 * execute) and validate replies the same way.
 */
import { describe, expect, it, vi } from 'vitest'
import type {
  AuditLog,
  FreeUpCandidates,
  HistoryRequest,
  MemoryHistory,
  MemorySnapshot,
  TerminateExecuteRequest,
  TerminateOutcome,
  TerminatePlan,
  TerminatePlanRequest,
} from '@space-lens/contract'

import { createHttpService } from '../src/http.ts'
import { createTauriService, TauriProblemError, type TauriPorts } from '../src/tauri.ts'

const GROUP_ID = 'grp_0123456789abcdef'
const PLAN_ID = 'tplan_0123456789ABCDEF'
const AUDIT_ID = 'aud_0123456789ABCDEF'

const SNAPSHOT: MemorySnapshot = {
  sequence: 3,
  sampledAt: '2026-10-18T10:05:00.000Z',
  intervalMs: 1000,
  platform: 'macos',
  breakdown: {
    totalBytes: 51539607552,
    usedBytes: 33500744909,
    freeBytes: 1073741824,
    availableBytes: 21474836480,
    appBytes: 18253611008,
    wiredBytes: 4294967296,
    compressedBytes: 7086696038,
    cachedFilesBytes: 3865470566,
    swapTotalBytes: 2147483648,
    swapUsedBytes: 3221225472,
  },
  pressure: { level: 'normal', source: 'kernel', availablePercent: 73.5 },
  groups: [
    {
      groupId: GROUP_ID,
      name: 'Dia',
      bundleId: 'com.dia.app',
      bundlePath: '/Applications/Dia.app',
      kind: 'app',
      mainPid: 4321,
      pids: [4321, 4322],
      cpuPercent: 12.5,
      footprintBytes: 426770432,
      residentBytes: 398458880,
      frontmost: false,
      lastFrontmostAt: '2026-10-18T10:00:00.000Z',
      startedAt: '2026-10-18T09:00:00.000Z',
      protection: null,
    },
  ],
  processes: [
    {
      pid: 4321,
      ppid: 1,
      startTimeUs: 1729180800000000,
      name: 'Dia',
      ownedByCurrentUser: true,
      groupId: GROUP_ID,
      kind: 'app',
      cpuPercent: 12.5,
      footprintBytes: 426770432,
      residentBytes: 398458880,
      virtualBytes: 5153960755,
      restricted: false,
      protection: null,
      command: '/Applications/Dia.app/Contents/MacOS/Dia',
      user: 'hk',
      state: 'sleeping',
      threads: 17,
      listeningPorts: [3080],
    },
  ],
  notes: ['cpuWarmingUp'],
}

const HISTORY_REQUEST: HistoryRequest = { groupIds: [GROUP_ID] }
const HISTORY: MemoryHistory = {
  stepMs: 1000,
  points: [
    {
      at: '2026-10-18T10:05:00.000Z',
      usedBytes: 33500744909,
      compressedBytes: 7086696038,
      swapUsedBytes: 3221225472,
      pressure: 'normal',
    },
  ],
  groups: [{ groupId: GROUP_ID, name: 'Dia', footprintBytes: [null, 426770432] }],
}

const CANDIDATES: FreeUpCandidates = {
  sequence: 3,
  pressure: { level: 'normal', source: 'kernel', availablePercent: 73.5 },
  observedMinutes: 6,
  candidates: [
    {
      groupId: GROUP_ID,
      name: 'Dia',
      footprintBytes: 426770432,
      reasons: [
        { code: 'large', bytes: 426770432, minutes: null, cpuPercent: null, count: null, fromBytes: null },
      ],
      risks: ['unsavedWorkMayBeLost'],
      preselected: false,
    },
  ],
  potentialReleaseBytes: 426770432,
}

const AUDIT_LOG: AuditLog = {
  records: [
    {
      auditId: AUDIT_ID,
      at: '2026-10-18T10:05:10.000Z',
      origin: 'manual',
      mode: 'graceful',
      method: 'appQuit',
      pid: 4321,
      startTimeUs: 1729180800000000,
      name: 'Dia',
      groupName: 'Dia',
      bundleId: 'com.dia.app',
      status: 'exited',
      footprintBytes: 426770432,
    },
  ],
}

const PLAN_REQUEST: TerminatePlanRequest = {
  targets: [{ kind: 'group', groupId: GROUP_ID }],
  mode: 'graceful',
  origin: 'manual',
}
const PLAN: TerminatePlan = {
  planId: PLAN_ID,
  mode: 'graceful',
  origin: 'manual',
  entries: [
    {
      pid: 4321,
      startTimeUs: 1729180800000000,
      name: 'Dia',
      groupId: GROUP_ID,
      footprintBytes: 426770432,
      method: 'appQuit',
      risks: ['appMayPromptToSave'],
      descendantCount: 1,
    },
  ],
  refused: [],
  expectedReleaseBytes: 426770432,
  createdAt: '2026-10-18T10:05:00.000Z',
  expiresAt: '2026-10-18T10:06:00.000Z',
}

const EXECUTE_REQUEST: TerminateExecuteRequest = { planId: PLAN_ID, confirm: true }
const OUTCOME: TerminateOutcome = {
  planId: PLAN_ID,
  results: [
    {
      pid: 4321,
      startTimeUs: 1729180800000000,
      name: 'Dia',
      status: 'exited',
      method: 'appQuit',
      errno: null,
    },
  ],
  releasedBytesEstimate: 426770432,
  auditIds: [AUDIT_ID],
}

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } })
}

describe('http memory service', () => {
  it('reads the snapshot from GET /api/v1/memory/snapshot and validates the reply', async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/memory/snapshot')
      expect(init?.method ?? 'GET').toBe('GET')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      return jsonResponse(SNAPSHOT)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    expect(await service.memorySnapshot!()).toEqual(SNAPSHOT)

    const malformed = createHttpService({
      baseUrl: 'http://localhost',
      getToken: () => 'session',
      fetchImpl: (async () => jsonResponse({ ...SNAPSHOT, sequence: -1 })) as typeof fetch,
    })
    await expect(malformed.memorySnapshot!()).rejects.toThrow()
  })

  it('posts the history and candidates bodies and validates both replies', async () => {
    const seen: string[] = []
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      seen.push(path)
      if (path.endsWith('/api/v1/memory/history')) {
        expect(JSON.parse(init?.body as string)).toEqual(HISTORY_REQUEST)
        return jsonResponse(HISTORY)
      }
      expect(JSON.parse(init?.body as string)).toEqual({ exclude: [GROUP_ID] })
      return jsonResponse(CANDIDATES)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    expect(await service.memoryHistory!(HISTORY_REQUEST)).toEqual(HISTORY)
    expect(await service.memoryCandidates!({ exclude: [GROUP_ID] })).toEqual(CANDIDATES)
    expect(seen).toEqual([
      'http://localhost/api/v1/memory/history',
      'http://localhost/api/v1/memory/candidates',
    ])
  })

  it('reads the audit log from GET /api/v1/memory/audit', async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/memory/audit')
      expect(init?.method ?? 'GET').toBe('GET')
      return jsonResponse(AUDIT_LOG)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    expect(await service.memoryAudit!()).toEqual(AUDIT_LOG)
  })

  it('posts the plan and execute bodies to the frozen terminate paths', async () => {
    const seen: string[] = []
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      const path = String(input)
      expect(init?.method).toBe('POST')
      seen.push(path)
      if (path.endsWith('/api/v1/memory/terminate/plan')) {
        expect(JSON.parse(init?.body as string)).toEqual(PLAN_REQUEST)
        return jsonResponse(PLAN)
      }
      expect(path).toBe('http://localhost/api/v1/memory/terminate/execute')
      expect(JSON.parse(init?.body as string)).toEqual(EXECUTE_REQUEST)
      return jsonResponse(OUTCOME)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    expect(await service.memoryTerminatePlan!(PLAN_REQUEST)).toEqual(PLAN)
    expect(await service.memoryTerminateExecute!(EXECUTE_REQUEST)).toEqual(OUTCOME)
    expect(seen).toEqual([
      'http://localhost/api/v1/memory/terminate/plan',
      'http://localhost/api/v1/memory/terminate/execute',
    ])
  })

  it('rejects an out-of-contract execute outcome instead of forwarding it', async () => {
    const service = createHttpService({
      baseUrl: 'http://localhost',
      getToken: () => 'session',
      fetchImpl: (async () => jsonResponse({ ...OUTCOME, planId: 'not-a-plan' })) as typeof fetch,
    })
    await expect(service.memoryTerminateExecute!(EXECUTE_REQUEST)).rejects.toThrow()
  })

  it('exposes no reopenApp on the HTTP adapter (desktop-only host request)', () => {
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => null })
    expect('reopenApp' in service).toBe(false)
  })
})

/** One Tauri port pair that records the outgoing payloads and answers each call. */
function recordingPorts(results: unknown[]): { ports: TauriPorts; requests: unknown[] } {
  let receive: (message: unknown) => void = () => {}
  const requests: unknown[] = []
  const ports: TauriPorts = {
    createChannel(callback) {
      receive = callback
      return 'channel'
    },
    async invoke(command, payload) {
      if (command === 'sl_connect') return { sessionId: 'session_1' }
      requests.push({ command, ...payload })
      receive({ ok: true, result: results[requests.length - 1] })
      return undefined
    },
  }
  return { ports, requests }
}

describe('tauri memory service', () => {
  it('sends the closed memory read arms and returns validated results', async () => {
    const { ports, requests } = recordingPorts([SNAPSHOT, HISTORY, CANDIDATES, AUDIT_LOG])
    const service = createTauriService(ports)
    expect(await service.memorySnapshot!()).toEqual(SNAPSHOT)
    expect(await service.memoryHistory!(HISTORY_REQUEST)).toEqual(HISTORY)
    expect(await service.memoryCandidates!({ exclude: [GROUP_ID] })).toEqual(CANDIDATES)
    expect(await service.memoryAudit!()).toEqual(AUDIT_LOG)
    expect(requests).toEqual([
      { command: 'sl_read', sessionId: 'session_1', reply: 'channel', request: { method: 'memorySnapshot' } },
      {
        command: 'sl_read',
        sessionId: 'session_1',
        reply: 'channel',
        request: { method: 'memoryHistory', groupIds: [GROUP_ID] },
      },
      {
        command: 'sl_read',
        sessionId: 'session_1',
        reply: 'channel',
        request: { method: 'memoryCandidates', exclude: [GROUP_ID] },
      },
      { command: 'sl_read', sessionId: 'session_1', reply: 'channel', request: { method: 'memoryAudit' } },
    ])
  })

  it('sends the closed terminate submit arms and the reopen host request', async () => {
    const { ports, requests } = recordingPorts([PLAN, OUTCOME, { reopened: true }])
    const service = createTauriService(ports)
    expect(await service.memoryTerminatePlan!(PLAN_REQUEST)).toEqual(PLAN)
    expect(await service.memoryTerminateExecute!(EXECUTE_REQUEST)).toEqual(OUTCOME)
    expect(await service.reopenApp!({ auditId: AUDIT_ID })).toEqual({ reopened: true })
    expect(requests).toEqual([
      {
        command: 'sl_submit',
        sessionId: 'session_1',
        reply: 'channel',
        request: { kind: 'memoryTerminatePlan', targets: PLAN_REQUEST.targets, mode: 'graceful', origin: 'manual' },
      },
      {
        command: 'sl_submit',
        sessionId: 'session_1',
        reply: 'channel',
        request: { kind: 'memoryTerminateExecute', planId: PLAN_ID, confirm: true },
      },
      {
        command: 'sl_host_request',
        sessionId: 'session_1',
        reply: 'channel',
        request: { kind: 'reopenApp', auditId: AUDIT_ID },
      },
    ])
  })

  it('rejects a reply that does not satisfy the contract schema and keeps typed problems', async () => {
    const { ports } = recordingPorts([{ ...SNAPSHOT, platform: 'beos' }])
    await expect(createTauriService(ports).memorySnapshot!()).rejects.toThrow()

    let receive: (message: unknown) => void = () => {}
    const failing: TauriPorts = {
      createChannel(callback) {
        receive = callback
        return 'channel'
      },
      async invoke(command) {
        if (command === 'sl_connect') return { sessionId: 'session_1' }
        receive({ ok: false, problem: { problem: { code: 'StalePlan', message: 'plan expired', retryable: false } } })
        return undefined
      },
    }
    const failure = await createTauriService(failing)
      .memoryTerminateExecute!(EXECUTE_REQUEST)
      .catch((error: unknown) => error)
    expect(failure).toBeInstanceOf(TauriProblemError)
    expect(failure).toMatchObject({ code: 'StalePlan' })
  })

  it('keeps execute on the default one-minute reply timeout', async () => {
    vi.useFakeTimers()
    try {
      let receive: (message: unknown) => void = () => {}
      const ports: TauriPorts = {
        createChannel(callback) {
          receive = callback
          return 'channel'
        },
        async invoke(command) {
          return command === 'sl_connect' ? { sessionId: 'session_1' } : undefined
        },
      }
      const settled = createTauriService(ports)
        .memoryTerminateExecute!(EXECUTE_REQUEST)
        .then(
          () => 'resolved' as const,
          (error: unknown) => error,
        )
      await vi.advanceTimersByTimeAsync(61_000)
      const outcome = await settled
      expect(outcome).toBeInstanceOf(Error)
      // A reply arriving after the timeout is ignored.
      receive({ ok: true, result: OUTCOME })
      expect(await settled).toBeInstanceOf(Error)
    } finally {
      vi.useRealTimers()
    }
  })
})
