import { mkdtempSync, mkdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import http from 'node:http'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { startServe, type RunningServer } from '../src/server.ts'
import { HOSTED_PASSWORD_ENV } from '../src/config.ts'
import type { TrashPort } from '../src/trash.ts'

function fakeTrash(): TrashPort & { trashed: string[] } {
  return {
    trashed: [],
    async trash(paths) {
      const results = new Map<string, string | null>()
      for (const path of paths) {
        this.trashed.push(path)
        results.set(path, null)
      }
      return results
    },
  }
}

function requestRaw(
  port: number,
  path: string,
  headers: Record<string, string>,
): Promise<{ status: number | undefined }> {
  return new Promise((resolve, reject) => {
    const request = http.request({ host: '127.0.0.1', port, path, headers, method: 'GET' }, (response) => {
      response.resume()
      response.on('end', () => resolve({ status: response.statusCode }))
    })
    request.on('error', reject)
    request.end()
  })
}

function pairTicket(pairingUrl: string): string {
  const url = new URL(pairingUrl)
  const ticket = url.searchParams.get('pair')
  if (ticket === null) throw new Error(`no pair ticket in ${pairingUrl}`)
  return ticket
}

describe('http host', () => {
  // realpath: the engine scans the resolved path (macOS /var → /private/var)
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-http-')))
  mkdirSync(join(root, 'data'))
  writeFileSync(join(root, 'data', 'file.txt'), 'content')
  writeFileSync(join(root, '.gitignore'), 'secret\n')
  mkdirSync(join(root, 'secret'))
  let server: RunningServer
  let token = ''
  let origin: string

  beforeAll(async () => {
    server = await startServe({
      port: 0,
      roots: [root],
      allowCleanup: true,
      maxTotalScans: 4,
      trash: fakeTrash(),
    })
    origin = server.baseUrl
  })

  afterAll(async () => {
    await server.stop()
    rmSync(root, { recursive: true, force: true })
  })

  const api = async (path: string, init?: RequestInit): Promise<Response> =>
    fetch(`${origin}${path}`, {
      ...init,
      headers: token === '' ? init?.headers : { authorization: `Bearer ${token}`, ...(init?.headers ?? {}) },
    })

  it('answers health without auth', async () => {
    const response = await fetch(`${origin}/health`)
    expect(response.status).toBe(200)
    const body = (await response.json()) as { status: string; apiMajor: number }
    expect(body.status).toBe('ok')
    expect(body.apiMajor).toBe(1)
  })

  it('pairs the terminal-minted ticket for its origin', async () => {
    const pairingUrl = server.mintPairingUrl()
    expect(pairingUrl.startsWith(origin)).toBe(true)
    const response = await fetch(`${origin}/api/v1/session/exchange`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', origin },
      body: JSON.stringify({ ticket: pairTicket(pairingUrl) }),
    })
    expect(response.status).toBe(200)
    const session = (await response.json()) as { token: string; scopes: string[] }
    token = session.token
    expect(session.scopes).toContain('scan:start')
    expect(session.scopes).toContain('cleanup:execute')
  })

  it('refuses api calls without a bearer token', async () => {
    const response = await fetch(`${origin}/api/v1/capabilities`)
    expect(response.status).toBe(401)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Unauthenticated')
  })

  it('refuses a wrong Host header and a foreign origin', async () => {
    const hostRefused = await requestRaw(server.port, '/health', { host: 'evil.example' })
    expect(hostRefused.status).toBe(400)
    const originRefused = await fetch(`${origin}/health`, { headers: { origin: 'https://evil.example' } })
    expect(originRefused.status).toBe(403)
  })

  it('refuses spoofed forwarded addresses outside the allowlist when the proxy is trusted', async () => {
    // restart with a restrictive allowlist + trustProxy to exercise the deny path
    const strict = await startServe({ port: 0, roots: [root], allowCidr: ['10.0.0.0/8'], trustProxy: true })
    try {
      const denied = await fetch(`${strict.baseUrl}/health`, { headers: { 'x-forwarded-for': '8.8.8.8' } })
      expect(denied.status).toBe(403)
      const allowed = await fetch(`${strict.baseUrl}/health`, { headers: { 'x-forwarded-for': '10.1.2.3' } })
      expect(allowed.status).toBe(200)
    } finally {
      await strict.stop()
    }
  })

  it('exposes capabilities and runs the scan flow end to end', async () => {
    const capabilities = await (await api('/api/v1/capabilities')).json()
    expect((capabilities as { cleanup: { mode: string } }).cleanup.mode).toBe('trash')

    const created = await api('/api/v1/scans', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ paths: [root], respectGitignore: true, ignoredMode: 'summarize' }),
    })
    expect(created.status).toBe(201)
    const session = (await created.json()) as { scanId: string }

    let status: { state: string; rootIds: string[] } = { state: 'scanning', rootIds: [] }
    for (let attempt = 0; attempt < 100; attempt += 1) {
      status = (await (await api(`/api/v1/scans/${session.scanId}`)).json()) as typeof status
      if (status.state === 'ready' || status.state === 'failed') break
      await new Promise((resolve) => setTimeout(resolve, 100))
    }
    expect(status.state).toBe('ready')
    expect(status.rootIds.length).toBe(1)

    const sliceResponse = await api('/api/v1/tree/slice', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ scanId: session.scanId, nodeId: status.rootIds[0], depth: 3, maxChildrenPerNode: 50 }),
    })
    expect(sliceResponse.status).toBe(200)
    const slice = (await sliceResponse.json()) as { tree: { children: Array<{ name: string; ignored: boolean }> } }
    const dataChild = slice.tree.children.find((child) => child.name === 'data')
    expect(dataChild).toBeDefined()

    const childrenResponse = await api('/api/v1/tree/children', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ scanId: session.scanId, nodeId: status.rootIds[0], offset: 0, limit: 10, sort: 'name' }),
    })
    expect(childrenResponse.status).toBe(200)

    // cleanup flow: plan the data directory
    const dataId = (await api('/api/v1/tree/children', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ scanId: session.scanId, nodeId: status.rootIds[0], offset: 0, limit: 100, sort: 'name' }),
    }).then((response) => response.json())) as { items: Array<{ id: string; name: string }> }
    const dataNodeId = dataId.items.find((item) => item.name === 'data')!.id
    const planResponse = await api('/api/v1/cleanup/plan', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ scanId: session.scanId, nodeIds: [dataNodeId] }),
    })
    expect(planResponse.status).toBe(200)
    const plan = (await planResponse.json()) as { planId: string; mode: string }
    expect(plan.mode).toBe('trash')

    // refuse to execute without the literal confirm
    const refused = await api('/api/v1/cleanup/execute', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ planId: plan.planId, confirm: false }),
    })
    expect(refused.status).toBe(400)
    expect(((await refused.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    // the refused request consumed the body, not the plan: execute properly, then expect the plan to be one-shot
    const outcome = await api('/api/v1/cleanup/execute', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ planId: plan.planId, confirm: true }),
    })
    expect(outcome.status).toBe(200)
    const result = (await outcome.json()) as { trashed: Array<{ path: string }>; failed: unknown[] }
    expect(result.failed).toEqual([])
    expect(result.trashed).toHaveLength(1)
    expect(result.trashed[0].path).toBe(join(root, 'data'))
  })

  it('streams events over sse with replay', async () => {
    const response = await api('/api/v1/events?since=0')
    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toContain('text/event-stream')
    const reader = response.body!.getReader()
    const decoder = new TextDecoder()
    let text = ''
    const deadline = Date.now() + 5000
    while (Date.now() < deadline && !text.includes('event: scan.completed')) {
      const chunk = await reader.read()
      if (chunk.done) break
      text += decoder.decode(chunk.value)
    }
    await reader.cancel()
    expect(text).toContain('event: hello')
    expect(text).toContain('scan.completed')
  })

  it('answers unknown api paths with a json problem, never the spa', async () => {
    const response = await api('/api/v1/definitely-not-a-route')
    expect(response.status).toBe(404)
    const body = (await response.json()) as { problem: { code: string } }
    expect(body.problem.code).toBe('NotFound')
    expect(response.headers.get('content-type')).toContain('application/json')
  })

  it('pairs machine-mode tickets only from clients without an origin', async () => {
    const machine = await startServe({ machine: true, roots: [root] })
    try {
      const pairingUrl = machine.mintPairingUrl()
      const ticket = pairTicket(pairingUrl)
      const good = await fetch(`${machine.baseUrl}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ ticket }),
      })
      expect(good.status).toBe(200)
      const second = machine.mintPairingUrl()
      const browserish = await fetch(`${machine.baseUrl}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: 'http://127.0.0.1:whatever' },
        body: JSON.stringify({ ticket: pairTicket(second) }),
      })
      // the origin gate refuses a browser origin before the ticket is even read
      expect(browserish.status).toBe(403)
    } finally {
      await machine.stop()
    }
  })

  it('enforces the hosted password for non-loopback ui origins', async () => {
    const previous = process.env[HOSTED_PASSWORD_ENV]
    process.env[HOSTED_PASSWORD_ENV] = 'a-long-enough-password'
    try {
      const hosted = await startServe({
        port: 0,
        roots: [root],
        uiOrigins: ['https://spacelens.example.workers.dev'],
      })
      try {
        const pairingUrl = hosted.mintPairingUrl()
        expect(pairingUrl.startsWith('https://spacelens.example.workers.dev')).toBe(true)
        const ticket = pairTicket(pairingUrl)
        const wrong = await fetch(`${hosted.baseUrl}/api/v1/session/exchange`, {
          method: 'POST',
          headers: { 'content-type': 'application/json', origin: 'https://spacelens.example.workers.dev' },
          body: JSON.stringify({ ticket }),
        })
        expect(wrong.status).toBe(401)
        const ticket2 = hosted.mintPairingUrl()
        const right = await fetch(`${hosted.baseUrl}/api/v1/session/exchange`, {
          method: 'POST',
          headers: { 'content-type': 'application/json', origin: 'https://spacelens.example.workers.dev' },
          body: JSON.stringify({ ticket: pairTicket(ticket2), password: 'a-long-enough-password' }),
        })
        expect(right.status).toBe(200)
      } finally {
        await hosted.stop()
      }
    } finally {
      if (previous === undefined) delete process.env[HOSTED_PASSWORD_ENV]
      else process.env[HOSTED_PASSWORD_ENV] = previous
    }
  })
})
