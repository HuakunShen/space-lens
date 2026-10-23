import { describe, expect, it } from 'vitest'

import { ServiceError, createHttpService } from '../src/http.ts'

function jsonResponse(payload: unknown, status: number): Response {
  return new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } })
}

describe('http service', () => {
  it('loads authenticated scan lists and rejects malformed status payloads', async () => {
    const status = {
      scanId: 'scan_abcdefgh',
      state: 'ready',
      message: '',
      progress: null,
      currentPath: null,
      bytesScanned: 4096,
      entriesScanned: 2,
      rootIds: ['node_root'],
      label: 'local',
      updatedAt: '2026-10-02T08:00:00.000Z',
    }
    let body: unknown = { scans: [status] }
    const service = createHttpService({
      baseUrl: 'http://localhost',
      getToken: () => 'session',
      fetchImpl: (async (url, init) => {
        expect(String(url)).toBe('http://localhost/api/v1/scans')
        expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
        return jsonResponse(body, 200)
      }) as typeof fetch,
    })
    expect(await service.listScans()).toEqual([status])
    body = { scans: [{ ...status, state: 'unknown' }] }
    await expect(service.listScans()).rejects.toThrow()
  })
  it('opens a ticket-less dev session on demand', async () => {
    const service = createHttpService({
      baseUrl: 'http://localhost',
      getToken: () => null,
      fetchImpl: (async (input: RequestInfo | URL, init?: RequestInit) => {
        expect(String(input)).toBe('http://localhost/api/v1/session/dev')
        expect(init?.method).toBe('POST')
        return jsonResponse(
          { token: 'sls_dev', sessionId: 'sess_dev', expiresAt: '2026-10-18T00:00:00.000Z', scopes: ['scan:read'] },
          200,
        )
      }) as typeof fetch,
    })

    const session = await service.devSession()
    expect(session.token).toBe('sls_dev')
  })
  it('posts the reveal request with authentication and returns the flag', async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/host/reveal')
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      expect(JSON.parse(init?.body as string)).toEqual({ path: '/tmp/a', action: 'open' })
      return jsonResponse({ revealed: true }, 200)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    expect(await service.revealPath({ path: '/tmp/a', action: 'open' })).toEqual({ revealed: true })
  })
  it('posts discovery paging with authentication and returns totals', async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/discovery')
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      expect(JSON.parse(init?.body as string)).toEqual({
        scanId: 'scan_abcdefgh',
        kind: 'large-files',
        minSize: 100,
        offset: 2,
        limit: 1,
      })
      return jsonResponse(
        { scanId: 'scan_abcdefgh', kind: 'large-files', items: [], total: 2, totalSize: 600, offset: 2, limit: 1 },
        200,
      )
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    const page = await service.discover({
      scanId: 'scan_abcdefgh',
      kind: 'large-files',
      minSize: 100,
      offset: 2,
      limit: 1,
    })
    expect(page.total).toBe(2)
    expect(page.totalSize).toBe(600)
  })
  it('sends the bearer token when present and maps problems to ServiceError', async () => {
    let seenAuthorization: string | undefined
    let seenPath = ''
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      seenPath = String(input)
      seenAuthorization = (init?.headers as Record<string, string>)?.authorization
      if (String(input).endsWith('/api/v1/scans')) {
        return jsonResponse({ problem: { code: 'Forbidden', message: 'read-only host', retryable: false } }, 403)
      }
      return jsonResponse({ status: 'ok', serviceInstanceId: 'inst_x', apiMajor: 1, contractVersion: '1.0.0' }, 200)
    }) as typeof fetch

    let token: string | null = 'sls_testtoken'
    const service = createHttpService({
      baseUrl: 'http://127.0.0.1:9420/',
      getToken: () => token,
      fetchImpl,
    })
    const health = await service.health()
    expect(health.status).toBe('ok')
    expect(seenAuthorization).toBe('Bearer sls_testtoken')

    token = null
    const error = await service
      .startScan({ paths: ['/tmp'], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' })
      .catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ServiceError)
    expect(seenAuthorization).toBeUndefined()
    expect(seenPath).toBe('http://127.0.0.1:9420/api/v1/scans')
    const serviceError = error as ServiceError
    expect(serviceError.code).toBe('Forbidden')
    expect(serviceError.status).toBe(403)
  })
  it('posts app audit requests and decodes the report with its audit id', async () => {
    const report = {
      platform: 'macos',
      generatedAt: '2026-10-13T00:00:00.000Z',
      apps: [],
      totalApps: 0,
      truncated: false,
      auditId: 'audit_abcdef1234567890abcdef',
    }
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/apps/audit')
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      expect(JSON.parse(init?.body as string)).toEqual({ minSize: 0, includeHeuristic: true, limit: 10 })
      return jsonResponse(report, 200)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    const decoded = await service.appAudit({ minSize: 0, includeHeuristic: true, limit: 10 })
    expect(decoded.auditId).toBe('audit_abcdef1234567890abcdef')
    expect(decoded.totalApps).toBe(0)
  })
  it('posts app icon requests and decodes the data url', async () => {
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/apps/icon')
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      expect(JSON.parse(init?.body as string)).toEqual({ appPath: '/Applications/Foo.app' })
      return jsonResponse({ dataUrl: 'data:image/png;base64,AAAA' }, 200)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    const icon = await service.appIcon({ appPath: '/Applications/Foo.app' })
    expect(icon.dataUrl).toBe('data:image/png;base64,AAAA')
  })
  it('reads the managed ai config and posts chat through the host', async () => {
    const calls: string[] = []
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      calls.push(`${init?.method ?? 'GET'} ${String(input)}`)
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      if (String(input).endsWith('/api/v1/ai/config')) {
        return jsonResponse(
          {
            baseUrl: 'https://api.example.com/v1',
            model: 'glm-4.6',
            hasKey: true,
            disableThinking: false,
            managed: true,
          },
          200,
        )
      }
      expect(init?.method).toBe('POST')
      expect(JSON.parse(init?.body as string)).toEqual({ messages: [{ role: 'user', content: 'hi' }] })
      return jsonResponse({ id: 'cmpl_1', choices: [] }, 200)
    }) as typeof fetch
    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })

    const view = await service.aiConfig()
    expect(view).toEqual({
      baseUrl: 'https://api.example.com/v1',
      model: 'glm-4.6',
      hasKey: true,
      disableThinking: false,
      managed: true,
    })
    const reply = (await service.aiChat({ messages: [{ role: 'user', content: 'hi' }] })) as { id: string }
    expect(reply.id).toBe('cmpl_1')
    expect(calls).toEqual(['GET http://localhost/api/v1/ai/config', 'POST http://localhost/api/v1/ai/chat'])
  })
})
