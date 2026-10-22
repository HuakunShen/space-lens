/**
 * Tests for the "open a scanned path in the host's file manager" route.
 *
 * The path is data, never a command: the host resolves it against the roots
 * it serves and refuses anything outside them. The launcher is injected here,
 * so a test never opens a real Finder window.
 */

import { mkdtempSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import { AuthService } from '../src/auth.ts'
import { buildApp, type HonoApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import { fileManagerTarget, hostPlatform, revealPath } from '../src/reveal.ts'
import { ScanManager } from '../src/scan-store.ts'

const HOST = '127.0.0.1:9421'

const tempRoots: string[] = []

interface Harness {
  app: HonoApp
  root: string
  launched: { command: string; args: string[] }[]
  token: string
}

function buildRevealApp(scopes: string[] = ['scan:read', 'host:reveal']): Harness {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-host-')))
  tempRoots.push(root)
  const config = resolveServeConfig({ port: 9421, roots: [root] })
  const auth = new AuthService({
    serviceInstanceId: 'inst_reveal',
    ticketTtlMs: 60_000,
    sessionTtlMs: 3_600_000,
    hostedPassword: null,
    scopes,
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
  const launched: { command: string; args: string[] }[] = []
  const app = buildApp({
    config,
    auth,
    scans,
    events,
    assets: async () => new Response('no ui', { status: 404 }),
    health: { status: 'ok', serviceInstanceId: 'inst_reveal', apiMajor: API_MAJOR, contractVersion: CONTRACT_VERSION },
    capabilities: {
      apiMajor: API_MAJOR,
      contractVersion: CONTRACT_VERSION,
      scan: { start: true, cancel: true, discovery: true, maxConcurrent: 2 },
      cleanup: { plan: false, execute: false, mode: 'none' },
      host: { folderPicker: false, pathReveal: true, platform: hostPlatform() },
      icloud: 'unavailable',
    },
    allowlist: createClientAllowlist([]),
    revealLauncher: (target) => {
      launched.push({ command: target.command, args: target.args })
    },
  })
  const ticket = auth.mintTicket('', false)
  const exchanged = auth.exchange(ticket, { origin: null })
  if (!exchanged.ok) throw new Error('exchange failed')
  return { app, root, launched, token: exchanged.session.token }
}

function post(app: HonoApp, token: string | null, body: unknown): Promise<Response> {
  return Promise.resolve(
    app.request(`http://${HOST}/api/v1/host/reveal`, {
      method: 'POST',
      headers: {
        host: HOST,
        'content-type': 'application/json',
        ...(token === null ? {} : { authorization: `Bearer ${token}` }),
      },
      body: JSON.stringify(body),
    }),
  )
}

afterAll(() => {
  for (const root of tempRoots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('revealing a scanned path in the host file manager', () => {
  it('needs a bearer token', async () => {
    const { app } = buildRevealApp()
    const response = await post(app, null, { path: '/tmp' })
    expect(response.status).toBe(401)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Unauthenticated')
  })

  it('needs the host:reveal scope', async () => {
    const { app, token } = buildRevealApp(['scan:read'])
    const response = await post(app, token, { path: '/tmp' })
    expect(response.status).toBe(403)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
  })

  it('opens a folder inside the served roots', async () => {
    const { app, root, launched, token } = buildRevealApp()
    const folder = join(root, 'folder')
    mkdirSync(folder)
    const response = await post(app, token, { path: folder })
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ revealed: true })
    expect(launched).toHaveLength(1)
    expect(launched[0]?.args).toContain(folder)
  })

  it('reveals by default and opens a folder on request', async () => {
    const { app, root, launched, token } = buildRevealApp()
    const folder = join(root, 'folder')
    mkdirSync(folder)
    expect((await post(app, token, { path: folder })).status).toBe(200)
    expect((await post(app, token, { path: folder, action: 'open' })).status).toBe(200)
    expect(launched).toHaveLength(2)
    // reveal: the parent folder with the item picked out; open: the folder.
    expect(launched[0]?.args).toContain('-R')
    expect(launched[1]?.args).not.toContain('-R')
    expect(launched[1]?.args).toContain(folder)
  })

  it('refuses an unknown action', async () => {
    const { app, root, token } = buildRevealApp()
    const response = await post(app, token, { path: root, action: 'delete' })
    expect(response.status).toBe(400)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
  })

  it('refuses a path outside the served roots', async () => {
    const { app, root, launched, token } = buildRevealApp()
    const outside = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-outside-')))
    tempRoots.push(outside)
    const response = await post(app, token, { path: outside })
    expect(response.status).toBe(403)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
    expect(launched).toHaveLength(0)
    // The served root itself is not a sibling of the outside path.
    expect(root).not.toBe(outside)
  })

  it('refuses a malformed body', async () => {
    const { app, token } = buildRevealApp()
    for (const body of [{}, { path: '' }, { path: 42 }, { path: '/tmp', extra: 1 }]) {
      const response = await post(app, token, body)
      expect(response.status).toBe(400)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    }
  })

  it('answers NotFound for a path that is gone', async () => {
    const { app, root, token } = buildRevealApp()
    const response = await post(app, token, { path: join(root, 'missing') })
    expect(response.status).toBe(404)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('NotFound')
  })
})

describe('the reveal launcher', () => {
  it('reveals the item in its parent folder and selects it', () => {
    expect(fileManagerTarget('macos', '/tmp/a', true, 'reveal')).toEqual({ command: 'open', args: ['-R', '/tmp/a'] })
    expect(fileManagerTarget('macos', '/tmp/a', false, 'reveal')).toEqual({ command: 'open', args: ['-R', '/tmp/a'] })
    expect(fileManagerTarget('windows', 'C:\\a', false, 'reveal')).toEqual({
      command: 'explorer.exe',
      args: ['/select,C:\\a'],
    })
    expect(fileManagerTarget('linux', '/tmp/a/b', false, 'reveal')).toEqual({
      command: 'xdg-open',
      args: ['/tmp/a'],
    })
  })

  it('opens a folder in the file manager itself', () => {
    expect(fileManagerTarget('macos', '/tmp/a', true, 'open')).toEqual({ command: 'open', args: ['/tmp/a'] })
    expect(fileManagerTarget('windows', 'C:\\a', true, 'open')).toEqual({ command: 'explorer.exe', args: ['C:\\a'] })
    expect(fileManagerTarget('linux', '/tmp/a', true, 'open')).toEqual({ command: 'xdg-open', args: ['/tmp/a'] })
  })

  it('refuses to open a file and refuses an unknown platform', () => {
    expect(() => fileManagerTarget('macos', '/tmp/a', false, 'open')).toThrow()
    expect(() => fileManagerTarget('unknown', '/tmp/a', true, 'reveal')).toThrow()
  })

  it('refuses paths outside the roots before launching anything', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-unit-')))
    tempRoots.push(root)
    const launched: { command: string; args: string[] }[] = []
    const outcome = await revealPath('/etc', [root], {
      platform: 'macos',
      launch: (target) => {
        launched.push({ command: target.command, args: target.args })
      },
    })
    expect(outcome.ok).toBe(false)
    expect(outcome.ok === false && outcome.reason).toBe('forbidden')
    expect(launched).toHaveLength(0)
  })

  it('refuses a relative path', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-relative-')))
    tempRoots.push(root)
    const outcome = await revealPath('relative/path', [root], { platform: 'macos', launch: () => {} })
    expect(outcome.ok).toBe(false)
    expect(outcome.ok === false && outcome.reason).toBe('invalid')
  })

  it('reports a file manager that cannot be started', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-launcher-')))
    tempRoots.push(root)
    const outcome = await revealPath(root, [root], {
      platform: 'macos',
      launch: () => {
        throw new Error('spawn open ENOENT')
      },
    })
    expect(outcome.ok).toBe(false)
    expect(outcome.ok === false && outcome.reason).toBe('unavailable')
  })

  it('refuses to open a file in the file manager', async () => {
    const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-reveal-open-file-')))
    tempRoots.push(root)
    const file = join(root, 'note.txt')
    writeFileSync(file, 'hello')
    const launched: { command: string; args: string[] }[] = []
    const outcome = await revealPath(file, [root], {
      platform: 'macos',
      action: 'open',
      launch: (target) => {
        launched.push({ command: target.command, args: target.args })
      },
    })
    expect(outcome.ok).toBe(false)
    expect(outcome.ok === false && outcome.reason).toBe('invalid')
    expect(launched).toHaveLength(0)
  })
})
