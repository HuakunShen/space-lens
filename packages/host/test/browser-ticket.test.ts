/**
 * Tests for the browser-ticket route (`POST /api/v1/session/ticket`).
 *
 * The CLI pairs with a machine-mode host over an SSH tunnel; a human then
 * wants the workbench in a browser pointed at the tunnel's local port. The
 * browser sends its own Origin, so the supervisor asks the host for a ticket
 * minted for exactly that loopback origin — and the host's browser gate
 * admits the origin only when that mint happened.
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
const BROWSER_ORIGIN = 'http://127.0.0.1:12399'

const tempRoots: string[] = []

function buildHarness(): { app: HonoApp; auth: AuthService } {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-browser-ticket-')))
  tempRoots.push(root)
  const config = resolveServeConfig({ port: 9421, roots: [root] })
  const auth = new AuthService({
    serviceInstanceId: 'inst_browser',
    ticketTtlMs: 60_000,
    sessionTtlMs: 3_600_000,
    hostedPassword: null,
    scopes: ['scan:read'],
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
    health: { status: 'ok', serviceInstanceId: 'inst_browser', apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION },
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
  app.rebindGate(9421)
  return { app, auth }
}

/** Pairs like the CLI does: machine ticket, no Origin header. */
async function supervisorSession(app: HonoApp, auth: AuthService): Promise<string> {
  const ticket = auth.mintTicket('', false)
  const response = await app.request(`http://${HOST}/api/v1/session/exchange`, {
    method: 'POST',
    headers: { host: HOST, 'content-type': 'application/json' },
    body: JSON.stringify({ ticket }),
  })
  expect(response.status).toBe(200)
  const session = (await response.json()) as { token: string }
  return session.token
}

afterAll(() => {
  for (const root of tempRoots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('browser tickets', () => {
  it('mints a ticket a loopback browser can spend, and only after minting', async () => {
    const { app, auth } = buildHarness()
    const token = await supervisorSession(app, auth)

    // Before any mint, a browser-origin pairing is refused by the gate.
    const refused = await app.request(`http://${HOST}/api/v1/session/exchange`, {
      method: 'POST',
      headers: { host: HOST, origin: BROWSER_ORIGIN, 'content-type': 'application/json' },
      body: JSON.stringify({ ticket: auth.mintTicket(BROWSER_ORIGIN, false) }),
    })
    expect(refused.status).toBe(403)

    const minted = await app.request(`http://${HOST}/api/v1/session/ticket`, {
      method: 'POST',
      headers: { host: HOST, authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ origin: BROWSER_ORIGIN }),
    })
    expect(minted.status).toBe(200)
    const browser = (await minted.json()) as { ticket: string; url: string }
    expect(browser.url).toBe(`${BROWSER_ORIGIN}/?pair=${browser.ticket}`)

    // The browser pairs with its own Origin header.
    const exchanged = await app.request(`http://${HOST}/api/v1/session/exchange`, {
      method: 'POST',
      headers: { host: HOST, origin: BROWSER_ORIGIN, 'content-type': 'application/json' },
      body: JSON.stringify({ ticket: browser.ticket }),
    })
    expect(exchanged.status).toBe(200)
    const browserSession = (await exchanged.json()) as { token: string; scopes: string[] }
    expect(browserSession.token).toMatch(/^sls_/)
    expect(browserSession.scopes).toContain('scan:read')

    // And the browser token works on the API.
    const capabilities = await app.request(`http://${HOST}/api/v1/capabilities`, {
      headers: { host: HOST, authorization: `Bearer ${browserSession.token}` },
    })
    expect(capabilities.status).toBe(200)
  })

  it('refuses unauthenticated supervisors and non-loopback origins', async () => {
    const { app, auth } = buildHarness()
    const token = await supervisorSession(app, auth)

    const anonymous = await app.request(`http://${HOST}/api/v1/session/ticket`, {
      method: 'POST',
      headers: { host: HOST, 'content-type': 'application/json' },
      body: JSON.stringify({ origin: BROWSER_ORIGIN }),
    })
    expect(anonymous.status).toBe(401)

    const remote = await app.request(`http://${HOST}/api/v1/session/ticket`, {
      method: 'POST',
      headers: { host: HOST, authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ origin: 'http://example.com:8080' }),
    })
    expect(remote.status).toBe(400)

    const httpsOrigin = await app.request(`http://${HOST}/api/v1/session/ticket`, {
      method: 'POST',
      headers: { host: HOST, authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      body: JSON.stringify({ origin: 'https://127.0.0.1:12399' }),
    })
    expect(httpsOrigin.status).toBe(400)
  })
})
