/**
 * Tests for the local session route (`POST /api/v1/session/local`).
 *
 * A page served by this host from the loopback interface is already on the
 * machine that owns the data, so it opens a session without a copied ticket:
 * that is what makes a reload or a second tab work. The trust boundary is the
 * peer address — a request from anywhere else is refused, and a hosted host
 * (password) or `--no-local-session` turns the route off entirely.
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

const HOST = '127.0.0.1:9421'
const tempRoots: string[] = []

interface Harness {
  app: HonoApp
  root: string
}

interface PeerEnv {
  incoming: { socket: { remoteAddress: string } }
}

function buildAppWith(options: {
  localSession?: boolean
  hostedPassword?: string | null
  /** Allow a wider network through the client gate, to reach the route's own check. */
  allowCidr?: string[]
}): Harness {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-local-session-')))
  tempRoots.push(root)
  const config = resolveServeConfig({
    port: 9421,
    roots: [root],
    ...(options.localSession === undefined ? {} : { localSession: options.localSession }),
    ...(options.allowCidr === undefined ? {} : { allowCidr: options.allowCidr }),
  })
  const auth = new AuthService({
    serviceInstanceId: 'inst_local',
    ticketTtlMs: 60_000,
    sessionTtlMs: 3_600_000,
    hostedPassword: options.hostedPassword ?? null,
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
    health: {
      status: 'ok',
      serviceInstanceId: 'inst_local',
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
    },
    capabilities: {
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
      scan: { start: true, cancel: true, discovery: true, maxConcurrent: 2 },
      cleanup: { plan: false, execute: false, mode: 'none' },
      host: { folderPicker: false, pathList: true },
      icloud: 'unavailable',
    },
    allowlist: createClientAllowlist(options.allowCidr ?? []),
  })
  return { app, root }
}

function openLocalSession(app: HonoApp, peer: string): Promise<Response> {
  const env: PeerEnv = { incoming: { socket: { remoteAddress: peer } } }
  return Promise.resolve(
    app.request(
      `http://${HOST}/api/v1/session/local`,
      { method: 'POST', headers: { host: HOST } },
      env,
    ),
  )
}

afterAll(() => {
  for (const root of tempRoots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('local sessions', () => {
  it('is on by default for a loopback listener and mints a working token', async () => {
    const { app } = buildAppWith({})

    const response = await openLocalSession(app, '127.0.0.1')
    expect(response.status).toBe(200)
    const session = (await response.json()) as { token: string; sessionId: string; scopes: string[] }
    expect(session.token).toMatch(/^sls_/)
    expect(session.scopes).toContain('scan:read')

    const capabilities = await app.request(`http://${HOST}/api/v1/capabilities`, {
      headers: { host: HOST, authorization: `Bearer ${session.token}` },
    })
    expect(capabilities.status).toBe(200)
  })

  it('accepts an IPv6 loopback peer and a v4-mapped one', async () => {
    const { app } = buildAppWith({})

    expect((await openLocalSession(app, '::1')).status).toBe(200)
    expect((await openLocalSession(app, '::ffff:127.0.0.1')).status).toBe(200)
  })

  it('refuses a peer that is not loopback', async () => {
    // Widen the client gate so the request reaches the route itself rather
    // than being stopped by the network allowlist first.
    const { app } = buildAppWith({ allowCidr: ['192.168.0.0/16'] })

    const response = await openLocalSession(app, '192.168.6.250')
    expect(response.status).toBe(403)
    const body = (await response.json()) as { problem: { message: string } }
    expect(body.problem.message).toMatch(/loopback-only/)
  })

  it('answers Unavailable with --no-local-session', async () => {
    const { app } = buildAppWith({ localSession: false })

    const response = await openLocalSession(app, '127.0.0.1')
    expect(response.status).toBe(503)
    const body = (await response.json()) as { problem: { message: string } }
    expect(body.problem.message).toMatch(/local sessions are off/)
  })

  it('is off by default when the host is hosted with a password', () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-local-session-')))
    tempRoots.push(root)
    const config = resolveServeConfig({ port: 9421, roots: [root], hostedPassword: 'secret' })
    expect(config.localSession).toBe(false)

    const loopback = resolveServeConfig({ port: 9421, roots: [root] })
    expect(loopback.localSession).toBe(true)

    // A ticket-free session must never be reachable from another machine.
    expect(() => resolveServeConfig({ host: '0.0.0.0', allowLan: true, localSession: true })).toThrow(
      /local sessions require a loopback bind/,
    )
  })
})
