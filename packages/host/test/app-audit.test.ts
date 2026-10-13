import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import { AuthService } from '../src/auth.ts'
import { buildApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import { ScanManager } from '../src/scan-store.ts'
import { LOCAL_SCAN_BIN_ENV, resolveLocalScanBin } from '../src/local-process.ts'
import { startServe, type RunningServer } from '../src/server.ts'
import type { TrashPort } from '../src/trash.ts'

function fakeTrash(): TrashPort {
  return { trash: async (paths) => new Map(paths.map((path) => [path, null])) }
}

function pairToken(server: RunningServer): Promise<string> {
  const url = new URL(server.mintPairingUrl())
  const ticket = url.searchParams.get('pair')
  if (ticket === null) throw new Error('no ticket')
  return fetch(`${server.baseUrl}/api/v1/session/exchange`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', origin: server.baseUrl },
    body: JSON.stringify({ ticket }),
  })
    .then((response) => response.json())
    .then((session) => (session as { token: string }).token)
}

describe('app storage audit over HTTP', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-app-audit-host-')))
  let server: RunningServer
  let token = ''

  beforeAll(async () => {
    server = await startServe({ port: 0, roots: [root], trash: fakeTrash() })
    token = await pairToken(server)
  })

  afterAll(async () => {
    await server.stop()
    rmSync(root, { recursive: true, force: true })
  })

  const api = (path: string, init?: RequestInit) =>
    fetch(`${server.baseUrl}${path}`, {
      ...init,
      headers: token === '' ? init?.headers : { authorization: `Bearer ${token}`, ...(init?.headers ?? {}) },
    })

  it('requires a bearer token for the app routes', async () => {
    for (const path of ['/api/v1/apps/audit', '/api/v1/apps/icon']) {
      const response = await fetch(`${server.baseUrl}${path}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(path.endsWith('audit') ? {} : { appPath: '/Applications/Foo.app' }),
      })
      expect(response.status).toBe(401)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Unauthenticated')
    }
  })

  it('refuses sessions without the scan:read scope', async () => {
    const config = resolveServeConfig({ port: 9420, roots: [root] })
    const auth = new AuthService({
      serviceInstanceId: 'inst_app_audit_scope',
      ticketTtlMs: 60_000,
      sessionTtlMs: 3_600_000,
      hostedPassword: null,
      scopes: ['scan:start'],
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
      health: { status: 'ok', serviceInstanceId: 'inst_app_audit_scope', apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION },
      capabilities: {
        apiMajor: API_MAJOR,
        contractVersion: CONTRACT_VERSION,
        scan: { start: true, cancel: true, discovery: true, appAudit: true, maxConcurrent: 2 },
        cleanup: { plan: false, execute: false, mode: 'none' },
        host: { folderPicker: false },
        icloud: 'unavailable',
      },
      allowlist: createClientAllowlist([]),
    })
    const ticket = auth.mintTicket('', false)
    const exchanged = auth.exchange(ticket, { origin: null })
    if (!exchanged.ok) throw new Error('exchange failed')
    const scopedToken = exchanged.session.token
    for (const [path, body] of [
      ['/api/v1/apps/audit', {}],
      ['/api/v1/apps/icon', { appPath: '/Applications/Foo.app' }],
    ] as const) {
      const response = await app.request(`http://127.0.0.1:9420${path}`, {
        method: 'POST',
        headers: {
          host: '127.0.0.1:9420',
          authorization: `Bearer ${scopedToken}`,
          'content-type': 'application/json',
        },
        body: JSON.stringify(body),
      })
      expect(response.status).toBe(403)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
    }
    await scans.close()
  })

  it('rejects malformed audit and icon bodies', async () => {
    for (const body of [{ limit: 0 }, { limit: 1001 }, { minSize: -1 }, { extra: true }, { includeHeuristic: 'yes' }]) {
      const response = await api('/api/v1/apps/audit', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      })
      expect(response.status).toBe(400)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    }
    for (const body of [{}, { appPath: '' }, { appPath: 42 }, { appPath: '/a', extra: 1 }]) {
      const response = await api('/api/v1/apps/icon', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      })
      expect(response.status).toBe(400)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    }
  })

  it('audits a temp home live when the CLI resolves, else answers gracefully', async () => {
    let binary: string | null = null
    try {
      binary = resolveLocalScanBin()
    } catch {
      binary = null
    }
    if (binary === null) {
      // No release CLI in this environment: the capability stays off and the
      // routes answer the standard Unavailable problem (graceful path).
      const capabilities = (await (await api('/api/v1/capabilities')).json()) as {
        scan: { appAudit?: boolean }
      }
      expect(capabilities.scan.appAudit).toBe(false)
      const audit = await api('/api/v1/apps/audit', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({}),
      })
      expect(audit.status).toBe(503)
      expect(((await audit.json()) as { problem: { code: string } }).problem.code).toBe('Unavailable')
      const icon = await api('/api/v1/apps/icon', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ appPath: '/Applications/Foo.app' }),
      })
      // Binary missing reads as Unavailable before the icon gate runs.
      expect([404, 503]).toContain(icon.status)
      return
    }
    // Live path: a temp HOME keeps the audit hermetic; an empty home yields
    // a valid empty-apps report carrying platform/auditId/apps.
    const home = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-app-audit-home-')))
    const live = await startServe({
      port: 0,
      roots: [root],
      trash: fakeTrash(),
      localScanBin: binary,
      appAuditHome: home,
    })
    try {
      const liveToken = await pairToken(live)
      const capabilities = (await (
        await fetch(`${live.baseUrl}/api/v1/capabilities`, {
          headers: { authorization: `Bearer ${liveToken}` },
        })
      ).json()) as { scan: { appAudit?: boolean } }
      expect(capabilities.scan.appAudit).toBe(true)
      const response = await fetch(`${live.baseUrl}/api/v1/apps/audit`, {
        method: 'POST',
        headers: { authorization: `Bearer ${liveToken}`, 'content-type': 'application/json' },
        body: JSON.stringify({}),
      })
      expect(response.status).toBe(200)
      const report = (await response.json()) as {
        platform: string
        auditId?: string
        apps: unknown[]
        totalApps: number
      }
      expect(typeof report.platform).toBe('string')
      expect(Array.isArray(report.apps)).toBe(true)
      expect(typeof report.auditId).toBe('string')
      expect(report.auditId).toMatch(/^audit_[0-9a-zA-Z]{8,64}$/)
      // An icon for a path the CLI cannot load reads as the standard 404 problem.
      const icon = await fetch(`${live.baseUrl}/api/v1/apps/icon`, {
        method: 'POST',
        headers: { authorization: `Bearer ${liveToken}`, 'content-type': 'application/json' },
        body: JSON.stringify({ appPath: join(home, 'Missing.app') }),
      })
      expect(icon.status).toBe(404)
      expect(((await icon.json()) as { problem: { code: string } }).problem.code).toBe('NotFound')
    } finally {
      await live.stop()
      rmSync(home, { recursive: true, force: true })
    }
  })

  it('exposes the local scan bin env contract', () => {
    expect(LOCAL_SCAN_BIN_ENV).toBe('SPACLENS_LOCAL_SCAN_BIN')
  })
})
