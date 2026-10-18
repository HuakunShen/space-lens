/**
 * Tests for the host's memory HTTP routes and their feature/scope gates (T14 of
 * docs/memory-cleaner-plan.md, §4.4/§4.5). A fake `MemoryBridge` is injected
 * through `HostDeps`, so no child process, process scan or signal runs here:
 * every case exercises the route order — feature gate, scope, body validation,
 * bridge forwarding — and reply validation against the contract schemas.
 */

import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import { AuthService, type SessionRecord } from '../src/auth.ts'
import { buildApp, type HonoApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import type { MemoryBridge, MemoryCall } from '../src/memory.ts'
import { Problems } from '../src/problems.ts'
import { ScanManager } from '../src/scan-store.ts'
import { startServe } from '../src/server.ts'
import type { TrashPort } from '../src/trash.ts'

const GROUP_A = 'grp_0123456789abcdef'
const GROUP_B = 'grp_fedcba9876543210'
const PLAN = 'tplan_0123456789abcdef'
const AT = '2026-10-18T10:05:00.000Z'
const HOST = '127.0.0.1:9420'

const pressure = { level: 'normal', source: 'kernel', availablePercent: 55 }

const SNAPSHOT = {
  sequence: 7,
  sampledAt: AT,
  intervalMs: 1000,
  platform: 'macos',
  breakdown: {
    totalBytes: 1024,
    usedBytes: 512,
    freeBytes: 512,
    availableBytes: null,
    appBytes: null,
    wiredBytes: null,
    compressedBytes: null,
    cachedFilesBytes: null,
    swapTotalBytes: 0,
    swapUsedBytes: 0,
  },
  pressure,
  groups: [],
  processes: [],
  notes: [],
}
const HISTORY = { stepMs: 1000, points: [], groups: [] }
const CANDIDATES = { sequence: 7, pressure, observedMinutes: 0, candidates: [], potentialReleaseBytes: 0 }
const AUDIT = { records: [] }
const PLAN_REPLY = {
  planId: PLAN,
  mode: 'graceful',
  origin: 'manual',
  entries: [],
  refused: [],
  expectedReleaseBytes: 0,
  createdAt: AT,
  expiresAt: AT,
}
const OUTCOME = { planId: PLAN, results: [], releasedBytesEstimate: 0, auditIds: [] }

function fakeTrash(): TrashPort {
  return { trash: async (paths) => new Map(paths.map((path) => [path, null])) }
}

interface RecordedCall {
  sessionId: string
  call: MemoryCall
}

type FakeBridge = MemoryBridge & { calls: RecordedCall[] }

/** A bridge whose `call` records the forwarded call and returns `reply(call)`. */
function fakeBridge(reply: (call: MemoryCall) => unknown = defaultReply): FakeBridge {
  const calls: RecordedCall[] = []
  return {
    calls,
    hello: async () => ({ platform: 'macos', terminate: true, gracefulAppQuit: true }),
    call: async (sessionId, call) => {
      calls.push({ sessionId, call })
      return reply(call)
    },
    stop: async () => {},
  }
}

function defaultReply(call: MemoryCall): unknown {
  switch (call.method) {
    case 'snapshot':
      return SNAPSHOT
    case 'history':
      return HISTORY
    case 'candidates':
      return CANDIDATES
    case 'audit':
      return AUDIT
    case 'plan':
      return PLAN_REPLY
    case 'execute':
      return OUTCOME
  }
}

const tempRoots: string[] = []

interface Harness {
  app: HonoApp
  auth: AuthService
  bridge?: FakeBridge
  scans: ScanManager
}

function buildMemoryApp(
  options: {
    memory?: MemoryBridge
    scopes?: readonly string[]
    config?: Parameters<typeof resolveServeConfig>[0]
  } = {},
): Harness {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-memory-routes-')))
  tempRoots.push(root)
  const config = resolveServeConfig({ port: 9420, roots: [root], ...options.config })
  const auth = new AuthService({
    serviceInstanceId: 'inst_memory',
    ticketTtlMs: 60_000,
    sessionTtlMs: 3_600_000,
    hostedPassword: null,
    scopes: options.scopes ?? ['scan:read', 'memory:read', 'memory:terminate'],
  })
  const events = new EventRing()
  const scans = new ScanManager({
    events,
    trash: fakeTrash(),
    roots: config.roots,
    cleanupMode: 'none',
    maxConcurrent: 2,
    maxTotal: 4,
  })
  const app = buildApp({
    config,
    auth,
    scans,
    events,
    assets: async () => new Response('no ui', { status: 404 }),
    health: { status: 'ok', serviceInstanceId: 'inst_memory', apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION },
    capabilities: {
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
      scan: { start: true, cancel: true, discovery: true, maxConcurrent: 2 },
      cleanup: { plan: false, execute: false, mode: 'none' },
      host: { folderPicker: false },
      icloud: 'unavailable',
    },
    allowlist: createClientAllowlist([]),
    ...(options.memory === undefined ? {} : { memory: options.memory }),
  })
  return { app, auth, bridge: options.memory as FakeBridge | undefined, scans }
}

function exchange(auth: AuthService): SessionRecord {
  const exchanged = auth.exchange(auth.mintTicket('', false), { origin: null })
  if (!exchanged.ok) throw new Error('exchange failed')
  return exchanged.session
}

function api(app: HonoApp, token: string, path: string, init?: RequestInit): Promise<Response> {
  return Promise.resolve(
    app.request(`http://${HOST}${path}`, {
      ...init,
      headers: { host: HOST, authorization: `Bearer ${token}`, ...(init?.headers ?? {}) },
    }),
  )
}

afterAll(() => {
  for (const root of tempRoots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('memory routes', () => {
  it('refuses a request without a bearer token', async () => {
    const { app } = buildMemoryApp({ memory: fakeBridge() })
    const response = await app.request(`http://${HOST}/api/v1/memory/snapshot`, { headers: { host: HOST } })
    expect(response.status).toBe(401)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Unauthenticated')
  })

  it('answers Unavailable when the host was not started with --allow-memory', async () => {
    const { app, auth } = buildMemoryApp()
    const session = exchange(auth)
    const snapshot = await api(app, session.token, '/api/v1/memory/snapshot')
    expect(snapshot.status).toBe(503)
    expect(((await snapshot.json()) as { problem: { code: string } }).problem.code).toBe('Unavailable')
    const plan = await api(app, session.token, '/api/v1/memory/terminate/plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ targets: [{ kind: 'group', groupId: GROUP_A }], mode: 'graceful', origin: 'manual' }),
    })
    expect(plan.status).toBe(503)
    expect(((await plan.json()) as { problem: { code: string } }).problem.code).toBe('Unavailable')
  })

  it('refuses a session that lacks memory:read', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge, scopes: ['scan:read', 'memory:terminate'] })
    const session = exchange(auth)
    const response = await api(app, session.token, '/api/v1/memory/snapshot')
    expect(response.status).toBe(403)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
    expect(bridge.calls).toHaveLength(0)
  })

  it('refuses a session that lacks memory:terminate for a plan', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge, scopes: ['scan:read', 'memory:read'] })
    const session = exchange(auth)
    const response = await api(app, session.token, '/api/v1/memory/terminate/plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ targets: [{ kind: 'group', groupId: GROUP_A }], mode: 'graceful', origin: 'manual' }),
    })
    expect(response.status).toBe(403)
    expect(bridge.calls).toHaveLength(0)
  })

  it('rejects a bad body before the bridge is touched', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)
    const tooMany = {
      targets: Array.from({ length: 21 }, () => ({ kind: 'group', groupId: GROUP_A })),
      mode: 'graceful',
      origin: 'manual',
    }
    const response = await api(app, session.token, '/api/v1/memory/terminate/plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(tooMany),
    })
    expect(response.status).toBe(400)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    expect(bridge.calls).toHaveLength(0)
  })

  it('forwards the session id and returns a validated snapshot', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)
    const response = await api(app, session.token, '/api/v1/memory/snapshot')
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual(SNAPSHOT)
    expect(bridge.calls).toEqual([{ sessionId: session.sessionId, call: { method: 'snapshot' } }])
  })

  it('forwards history, candidates and audit with their parsed bodies', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)

    const history = await api(app, session.token, '/api/v1/memory/history', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ groupIds: [GROUP_A, GROUP_B] }),
    })
    expect(history.status).toBe(200)
    expect(await history.json()).toEqual(HISTORY)

    const candidates = await api(app, session.token, '/api/v1/memory/candidates', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ exclude: [GROUP_B] }),
    })
    expect(candidates.status).toBe(200)
    expect(await candidates.json()).toEqual(CANDIDATES)

    const audit = await api(app, session.token, '/api/v1/memory/audit')
    expect(audit.status).toBe(200)
    expect(await audit.json()).toEqual(AUDIT)

    expect(bridge.calls).toEqual([
      { sessionId: session.sessionId, call: { method: 'history', request: { groupIds: [GROUP_A, GROUP_B] } } },
      { sessionId: session.sessionId, call: { method: 'candidates', request: { exclude: [GROUP_B] } } },
      { sessionId: session.sessionId, call: { method: 'audit' } },
    ])
  })

  it('forwards plan and execute with the literal confirm', async () => {
    const bridge = fakeBridge()
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)

    const plan = await api(app, session.token, '/api/v1/memory/terminate/plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ targets: [{ kind: 'group', groupId: GROUP_A }], mode: 'graceful', origin: 'freeUp' }),
    })
    expect(plan.status).toBe(200)
    expect(await plan.json()).toEqual(PLAN_REPLY)

    const execute = await api(app, session.token, '/api/v1/memory/terminate/execute', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ planId: PLAN, confirm: true }),
    })
    expect(execute.status).toBe(200)
    expect(await execute.json()).toEqual(OUTCOME)

    expect(bridge.calls).toEqual([
      {
        sessionId: session.sessionId,
        call: {
          method: 'plan',
          request: { targets: [{ kind: 'group', groupId: GROUP_A }], mode: 'graceful', origin: 'freeUp' },
        },
      },
      { sessionId: session.sessionId, call: { method: 'execute', request: { planId: PLAN, confirm: true } } },
    ])
  })

  it('turns a malformed child reply into InternalError, never forwarding it', async () => {
    const bridge = fakeBridge(() => ({ not: 'a snapshot' }))
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)
    const response = await api(app, session.token, '/api/v1/memory/snapshot')
    expect(response.status).toBe(500)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InternalError')
  })

  it('propagates a bridge problem through the host problem helpers', async () => {
    const bridge = fakeBridge(() => {
      throw Problems.limit('The memory service is busy.')
    })
    const { app, auth } = buildMemoryApp({ memory: bridge })
    const session = exchange(auth)
    const response = await api(app, session.token, '/api/v1/memory/candidates', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ exclude: [] }),
    })
    expect(response.status).toBe(429)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('LimitExceeded')
  })
})

describe('memory capability wiring', () => {
  it('grants the memory scopes but omits the capability when the release CLI cannot be resolved', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-memory-cap-')))
    tempRoots.push(root)
    const server = await startServe({
      port: 0,
      roots: [root],
      allowMemory: true,
      allowProcessControl: true,
      localScanBin: join(root, 'does-not-exist', 'spacelens'),
    })
    try {
      const url = new URL(server.mintPairingUrl())
      const ticket = url.searchParams.get('pair')
      if (ticket === null) throw new Error('no ticket')
      const response = await fetch(`${server.baseUrl}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: server.baseUrl },
        body: JSON.stringify({ ticket }),
      })
      const session = (await response.json()) as { token: string; scopes: string[] }
      expect(session.scopes).toContain('memory:read')
      expect(session.scopes).toContain('memory:terminate')
      const capabilities = (await (
        await fetch(`${server.baseUrl}/api/v1/capabilities`, { headers: { authorization: `Bearer ${session.token}` } })
      ).json()) as { memory?: unknown }
      expect(capabilities.memory).toBeUndefined()
    } finally {
      await server.stop()
    }
  })
})
