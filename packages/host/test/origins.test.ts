// Unit tests for the Host/Origin gate. The gate is the only thing between a
// hostile web page and a loopback service, so its verdicts are pinned here
// rather than only through full-server requests.
import { describe, expect, it } from 'vitest'
import { authoritiesFor, checkOrigin, type OriginPolicy } from '../src/origins.ts'

const policy: OriginPolicy = {
  authorities: authoritiesFor('127.0.0.1', 8799),
  allowedOrigins: ['http://127.0.0.1:8799', 'http://localhost:8799'],
}

function check(overrides: Partial<Parameters<typeof checkOrigin>[0]> = {}): ReturnType<typeof checkOrigin> {
  return checkOrigin(
    {
      hostHeader: '127.0.0.1:8799',
      origin: null,
      secFetchSite: null,
      secFetchMode: null,
      ...overrides,
    },
    policy,
  )
}

describe('origin gate', () => {
  it('refuses a request addressed to another authority before anything else', () => {
    expect(check({ hostHeader: 'evil.example:8799' })).toBe('refused-host')
    expect(check({ hostHeader: null })).toBe('refused-host')
    expect(check({ hostHeader: 'localhost:8799', origin: 'https://evil.example' })).toBe('refused-origin')
  })

  it('requires a presented Origin to be on the allowlist, whatever the fetch metadata says', () => {
    expect(check({ origin: 'https://evil.example', secFetchSite: 'cross-site' })).toBe('refused-origin')
    expect(check({ origin: 'http://127.0.0.1:8799', secFetchSite: 'same-origin' })).toBe('ok')
    expect(check({ origin: 'http://localhost:8799', secFetchSite: 'same-site' })).toBe('ok')
  })

  it('refuses an origin-less cross-site subresource', () => {
    expect(check({ secFetchSite: 'cross-site', secFetchMode: 'cors' })).toBe('refused-cross-site')
    expect(check({ secFetchSite: 'cross-site', secFetchMode: 'no-cors' })).toBe('refused-cross-site')
    expect(check({ secFetchSite: 'cross-site', secFetchMode: 'same-origin' })).toBe('refused-cross-site')
  })

  it('lets a top-level navigation from another site load the app', () => {
    // Opening the workbench from a chat message or a bookmark: no Origin,
    // cross-site, mode=navigate. Refusing it showed a JSON error instead of
    // the app.
    expect(check({ secFetchSite: 'cross-site', secFetchMode: 'navigate' })).toBe('ok')
  })

  it('leaves same-site and non-browser clients alone', () => {
    expect(check({ secFetchSite: 'same-origin' })).toBe('ok')
    expect(check({ secFetchSite: 'none' })).toBe('ok')
    expect(check()).toBe('ok')
  })
})
