/**
 * Exercise the embedded transport and pairing against a real Space Lens server.
 *
 * Mirrors the Refyard plugin's dsh-plugin test: the pairing redirect must carry a
 * same-origin (root-relative) API path rather than the HTTP carrier authority, and
 * the proxy must redeem tickets through an Origin-stripping carrier without losing
 * the ticket/bearer checks or letting foreign origins through.
 */
import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { createServer, request, type IncomingMessage, type ServerResponse } from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { apply } from '../../../apps/dsh/src/host.ts'

describe('Harness embedded workbench transport', () => {
  // realpath: the server canonicalises the directory (macOS /var → /private/var)
  const scratch = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-dsh-')))
  let origin: string
  let handler: ((req: IncomingMessage, res: ServerResponse) => void | Promise<void>) | undefined
  const cleanup: (() => void)[] = []
  const server = createServer((req, res) => {
    if (handler === undefined) {
      res.writeHead(503)
      res.end()
      return
    }
    void handler(req, res)
  })

  beforeAll(async () => {
    await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
    const address = server.address()
    if (address === null || typeof address === 'string') throw new Error('no HTTP port')
    origin = `http://127.0.0.1:${address.port}`
    const sessionId = 'sess_test_transport'
    apply({
      logger: { info() {}, warn() {}, error() {} },
      get: () => ({
        port: address.port,
        register(route: {
          kind: 'exact' | 'prefix'
          path: string
          handler: (req: IncomingMessage, res: ServerResponse) => void | Promise<void>
        }) {
          handler = route.handler
          return () => {
            handler = undefined
          }
        },
      }),
      on(name: 'session/created' | 'session/event', listener: (session: { id?: unknown; header?: { cwd?: unknown } }, event?: unknown) => void) {
        // Teach the plugin which directory the session works in, as the Harness would.
        if (name === 'session/created' || name === 'session/event') {
          listener({ id: sessionId, header: { cwd: scratch } }, undefined)
        }
        return undefined
      },
      effect(install: () => void | (() => void)) {
        const dispose = install()
        if (dispose !== undefined) cleanup.push(dispose)
      },
    })
  })

  afterAll(async () => {
    cleanup.reverse().forEach((dispose) => dispose())
    await new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve())))
    rmSync(scratch, { recursive: true, force: true })
  })

  async function pairing(): Promise<URL> {
    const response = await fetch(`${origin}/space-lens/?dir=${encodeURIComponent(scratch)}`, {
      headers: { accept: 'text/html' },
      redirect: 'manual',
    })
    expect(response.status).toBe(302)
    return new URL(response.headers.get('location') ?? '', origin)
  }

  it('redirects with a same-origin API path rather than the HTTP carrier authority', async () => {
    const url = await pairing()
    const api = url.searchParams.get('api')
    expect(url.searchParams.get('pair')).toBeTruthy()
    expect(url.searchParams.get('autoscan')).toBe('1')
    // Root-relative: the frame stays on the renderer's transport, never an
    // absolute HTTP(S) address that would escape desktop's carrier.
    expect(api?.startsWith('http://') ?? true).toBe(false)
    expect(api?.startsWith('https://') ?? true).toBe(false)
    expect(api).toBe(`/space-lens/d/${encodeURIComponent(scratch)}`)
  })

  it('redeems through an Origin-stripping carrier without losing ticket/bearer checks', async () => {
    // Electron validates dsh-app Origin then strips it; this request matches that carrier.
    const url = await pairing()
    const api = url.searchParams.get('api')
    const ticket = url.searchParams.get('pair')
    const exchange = () =>
      fetch(`${origin}${api}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ticket }),
      })
    const response = await exchange()
    expect(response.status).toBe(200)
    const result: unknown = await response.json()
    if (typeof result !== 'object' || result === null || !('token' in result) || typeof result.token !== 'string')
      throw new Error('no bearer issued')
    expect(
      (
        await fetch(`${origin}${api}/api/v1/capabilities`, {
          headers: { authorization: `Bearer ${result.token}` },
        })
      ).status,
    ).toBe(200)
    expect((await exchange()).status).toBe(401)
    expect((await fetch(`${origin}${api}/api/v1/capabilities`)).status).toBe(401)
  })

  it('refuses foreign/opaque origins, foreign Host, and originless cross-site before mapping', async () => {
    const url = await pairing()
    const api = url.searchParams.get('api')
    const body = JSON.stringify({ ticket: url.searchParams.get('pair') })
    for (const headers of [
      { origin: 'https://evil.example' },
      { origin: 'null' },
      { host: 'evil.example' },
      { 'sec-fetch-site': 'cross-site' },
    ]) {
      // A rejected attempt must not consume the ticket or be rewritten as local.
      // Use raw HTTP: Fetch normalises security-sensitive Host/Fetch metadata.
      const status = await new Promise<number>((resolve, reject) => {
        const outgoing = request(
          `${origin}${api}/api/v1/session/exchange`,
          {
            method: 'POST',
            headers: { ...headers, 'content-type': 'application/json' },
          },
          (response) => {
            response.resume()
            resolve(response.statusCode ?? 0)
          },
        )
        outgoing.on('error', reject)
        outgoing.end(body)
      })
      expect(status, JSON.stringify(headers)).toBe(403)
    }
    expect(
      (
        await fetch(`${origin}${api}/api/v1/session/exchange`, {
          method: 'POST',
          headers: { origin, 'content-type': 'application/json' },
          body,
        })
      ).status,
    ).toBe(200)
  })
})
