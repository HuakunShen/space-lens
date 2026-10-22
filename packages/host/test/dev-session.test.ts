/**
 * Tests for the development-only session route (`--dev-open-session`).
 *
 * The pairing ticket is what proves a human read the terminal. The flag exists
 * so a dev server does not need a copied ticket on every reload; it is the
 * operator's explicit choice, it only works on a loopback bind, and without the
 * flag the route answers Unavailable so the pairing form stays the answer for a
 * normal host.
 */

import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import { AuthService } from '../src/auth.ts'
import { buildApp, type HonoApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import { ScanManager } from '../src/scan-store.ts'

const HOST = '127.0.0.1:9420'

const tempRoots: string[] = []

interface Harness {
  app: HonoApp
  root: string
}

function buildDevApp(devOpenSession: boolean): Harness {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-dev-session-')))
  tempRoots.push(root)
  const config = resolveServeConfig({ port: 9420, roots: [root], devOpenSession })
  const auth = new AuthService({
    serviceInstanceId: 'inst_dev',
    ticketTtlMs: 60_000,
    sessionTtlMs: 3_600_000,
    hostedPassword: null,
    scopes: ['scan:read', 'memory:read'],
  })
  const events = new EventRing()
  const scans = new ScanManager({
    events,
    trash: { trash: async (paths) => new Map(paths.map((path) => [path, null])) },
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
    health: { status: 'ok', serviceInstanceId: 'inst_dev', apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION },
    capabilities: {
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
      scan: { start: true, cancel: true, discovery: true, maxConcurrent: 2 },
      cleanup: { plan: false, execute: false, mode: 'none' },
      host: { folderPicker: false },
      icloud: 'unavailable',
    },
    allowlist: createClientAllowlist([]),
  })
  return { app, root }
}

function openDevSession(app: HonoApp): Promise<Response> {
  return Promise.resolve(
    app.request(`http://${HOST}/api/v1/session/dev`, { method: 'POST', headers: { host: HOST } }),
  )
}

afterAll(() => {
  for (const root of tempRoots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('dev sessions', () => {
  it('opens a working session without a ticket when the flag is on', async () => {
    const { app } = buildDevApp(true)

    const response = await openDevSession(app)
    expect(response.status).toBe(200)
    const session = (await response.json()) as { token: string; sessionId: string; scopes: string[] }
    expect(session.token).toMatch(/^sls_/)
    expect(session.sessionId).toMatch(/^sess_/)
    expect(session.scopes).toContain('scan:read')

    // The token is a real session, not a placeholder: a protected route answers.
    const capabilities = await app.request(`http://${HOST}/api/v1/capabilities`, {
      headers: { host: HOST, authorization: `Bearer ${session.token}` },
    })
    expect(capabilities.status).toBe(200)
  })

  it('hands out a distinct token per call', async () => {
    const { app } = buildDevApp(true)

    const first = (await (await openDevSession(app)).json()) as { token: string }
    const second = (await (await openDevSession(app)).json()) as { token: string }

    expect(first.token).not.toBe(second.token)
  })

  it('answers Unavailable when the flag is off', async () => {
    const { app } = buildDevApp(false)

    const response = await openDevSession(app)
    expect(response.status).toBe(503)
    const body = (await response.json()) as { problem: { code: string; message: string } }
    expect(body.problem.code).toBe('Unavailable')
    expect(body.problem.message).toMatch(/dev sessions are off/)
  })
})
