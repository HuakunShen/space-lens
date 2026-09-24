import { describe, expect, it } from 'vitest'

import { AuthService, hashHostedPassword } from '../src/auth.ts'

function makeService(overrides?: Partial<ConstructorParameters<typeof AuthService>[0]>): AuthService {
  return new AuthService({
    serviceInstanceId: 'inst_test',
    ticketTtlMs: 60_000,
    sessionTtlMs: 60_000,
    hostedPassword: null,
    scopes: ['scan:read', 'scan:start'],
    ...overrides,
  })
}

describe('auth service', () => {
  it('pairs a ticket for its origin exactly once', () => {
    const auth = makeService()
    const ticket = auth.mintTicket('http://127.0.0.1:9000', false)
    const first = auth.exchange(ticket, { origin: 'http://127.0.0.1:9000' })
    expect(first.ok).toBe(true)
    if (first.ok) {
      expect(first.session.token.startsWith('sls_')).toBe(true)
      expect(first.session.scopes).toContain('scan:read')
    }
    // single use
    const second = auth.exchange(ticket, { origin: 'http://127.0.0.1:9000' })
    expect(second).toMatchObject({ ok: false, reason: 'unknown' })
  })

  it('binds tickets to their origin', () => {
    const auth = makeService()
    const ticket = auth.mintTicket('http://127.0.0.1:9000', false)
    expect(auth.exchange(ticket, { origin: 'https://evil.example' })).toMatchObject({
      ok: false,
      reason: 'origin-mismatch',
    })
  })

  it('binds machine tickets to the empty origin only', () => {
    const auth = makeService()
    const ticket = auth.mintTicket('', false)
    expect(auth.exchange(ticket, { origin: null })).toMatchObject({ ok: true })
    const ticket2 = auth.mintTicket('', false)
    expect(auth.exchange(ticket2, { origin: 'http://127.0.0.1:9000' })).toMatchObject({
      ok: false,
      reason: 'origin-mismatch',
    })
  })

  it('expires tickets', () => {
    const auth = makeService({ ticketTtlMs: -1 })
    const ticket = auth.mintTicket('', false)
    expect(auth.exchange(ticket, { origin: null })).toMatchObject({ ok: false, reason: 'expired' })
  })

  it('requires the hosted password when set', () => {
    const hosted = hashHostedPassword('a-long-enough-password')
    const auth = makeService({ hostedPassword: hosted })
    // every attempt needs a fresh ticket: tickets are single-use regardless
    // of the verdict
    expect(auth.exchange(auth.mintTicket('', true), { origin: null })).toMatchObject({ ok: false, reason: 'password' })
    expect(auth.exchange(auth.mintTicket('', true), { origin: null, password: 'short' })).toMatchObject({
      ok: false,
      reason: 'password',
    })
    expect(
      auth.exchange(auth.mintTicket('', true), { origin: null, password: 'wrong-password-entirely' }),
    ).toMatchObject({ ok: false, reason: 'password' })
    expect(
      auth.exchange(auth.mintTicket('', true), { origin: null, password: 'a-long-enough-password' }),
    ).toMatchObject({ ok: true })
  })

  it('authorizes bearer sessions and expires them', () => {
    const auth = makeService({ sessionTtlMs: -1 })
    const ticket = auth.mintTicket('', false)
    const result = auth.exchange(ticket, { origin: null })
    expect(result.ok).toBe(true)
    if (!result.ok) return
    expect(auth.authorize(`Bearer ${result.session.token}`)).toBeNull()
  })
})
