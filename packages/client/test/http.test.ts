import { describe, expect, it } from 'vitest'

import { ServiceError, createHttpService } from '../src/http.ts'

function jsonResponse(payload: unknown, status: number): Response {
  return new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } })
}

describe('http service', () => {
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
    const error = await service.startScan({ paths: ['/tmp'], ignoreHidden: false, respectGitignore: true, ignoredMode: 'summarize' }).catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ServiceError)
    expect(seenAuthorization).toBeUndefined()
    expect(seenPath).toBe('http://127.0.0.1:9420/api/v1/scans')
    const serviceError = error as ServiceError
    expect(serviceError.code).toBe('Forbidden')
    expect(serviceError.status).toBe(403)
  })
})
