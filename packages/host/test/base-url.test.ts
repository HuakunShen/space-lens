import { describe, expect, it } from 'vitest'

import { normalizeBaseUrl, parseBaseUrl } from '../../../apps/web/src/lib/base-url.ts'

describe('service address', () => {
  it("defaults to this page's origin, which is the service that served it", () => {
    expect(parseBaseUrl({ queryApi: null, storedBaseUrl: null, pageOrigin: 'http://127.0.0.1:9420' })).toEqual({
      url: 'http://127.0.0.1:9420',
      sameOrigin: true,
    })
  })

  it('prefers an explicit ?api= override and says so', () => {
    expect(
      parseBaseUrl({
        queryApi: 'http://127.0.0.1:5000',
        storedBaseUrl: null,
        pageOrigin: 'http://127.0.0.1:9420',
      }),
    ).toEqual({ url: 'http://127.0.0.1:5000', sameOrigin: false })
  })

  it('keeps an embedded API on the page transport, even with a stale stored HTTP address', () => {
    // The DSH pairing redirect sets a root-relative api so the frame stays on the
    // renderer's transport (HTTP in web, dsh-app://app in desktop). A stored absolute
    // address from an earlier standalone visit must not win over it.
    expect(
      parseBaseUrl({
        queryApi: '/space-lens/d/%2Ftmp%2Fproject',
        storedBaseUrl: 'http://127.0.0.1:9420',
        pageOrigin: 'http://127.0.0.1:3080',
      }),
    ).toEqual({ url: '/space-lens/d/%2Ftmp%2Fproject', sameOrigin: true })
    expect(normalizeBaseUrl('/space-lens/')).toBe('/space-lens')
    expect(normalizeBaseUrl('/space-lens/?x=1#y')).toBe('/space-lens')
    expect(normalizeBaseUrl('/')).toBe('')
  })

  it('falls back to the page origin when an override is not a usable address', () => {
    expect(
      parseBaseUrl({ queryApi: 'not a url', storedBaseUrl: null, pageOrigin: 'http://127.0.0.1:9420' }),
    ).toEqual({ url: 'http://127.0.0.1:9420', sameOrigin: true })
  })

  it('uses a remembered address when the URL carries none', () => {
    expect(
      parseBaseUrl({
        queryApi: null,
        storedBaseUrl: 'http://127.0.0.1:9420',
        pageOrigin: 'https://app.example.test',
      }),
    ).toEqual({ url: 'http://127.0.0.1:9420', sameOrigin: false })
  })

  it('keeps a service path and normalises away the query and trailing slash', () => {
    expect(normalizeBaseUrl('http://127.0.0.1:9420/')).toBe('http://127.0.0.1:9420')
    expect(normalizeBaseUrl('http://127.0.0.1:9420/app/?x=1#y')).toBe('http://127.0.0.1:9420/app')
    expect(normalizeBaseUrl('http://127.0.0.1:3080/space-lens')).toBe('http://127.0.0.1:3080/space-lens')
  })

  it('rejects relative addresses that can escape or ambiguously change the mount', () => {
    for (const address of [
      '//evil.example/space-lens',
      '/\\\\evil.example/space-lens',
      '/space-lens\\\\other',
      '/space-lens/../api',
      '/space-lens/./api',
      '/space-lens/%2e%2e/api',
      '/space-lens/.%2e/api',
      '/space-lens/%2e./api',
      '/space-lens\n/api',
      '/space-lens\t/api',
      'space-lens',
    ])
      expect(normalizeBaseUrl(address)).toBeNull()
  })

  it('refuses a scheme that cannot carry an authenticated API', () => {
    expect(normalizeBaseUrl('file:///tmp/index.html')).toBeNull()
    expect(normalizeBaseUrl('ws://127.0.0.1:9420')).toBeNull()
    expect(normalizeBaseUrl('   ')).toBeNull()
    expect(normalizeBaseUrl(null)).toBeNull()
  })
})
