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
})
