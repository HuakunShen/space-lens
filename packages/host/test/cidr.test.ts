import { describe, expect, it } from 'vitest'

import { createClientAllowlist, normalizeRemoteAddress, validateCidrs } from '../src/cidr.ts'

describe('client allowlist', () => {
  it('always allows loopback and honors configured cidrs', () => {
    const allowlist = createClientAllowlist(['192.168.1.0/24'])
    expect(allowlist.check('127.0.0.1')).toBe(true)
    expect(allowlist.check('192.168.1.5')).toBe(true)
    expect(allowlist.check('192.168.2.5')).toBe(false)
    expect(allowlist.check('::1')).toBe(true)
  })

  it('maps ipv4-mapped ipv6 addresses back to ipv4', () => {
    const allowlist = createClientAllowlist(['10.0.0.0/8'])
    expect(allowlist.check('::ffff:10.1.2.3')).toBe(true)
    expect(allowlist.check('::ffff:8.8.8.8')).toBe(false)
  })

  it('treats bare addresses as single hosts', () => {
    const allowlist = createClientAllowlist(['192.168.7.9'])
    expect(allowlist.check('192.168.7.9')).toBe(true)
    expect(allowlist.check('192.168.7.10')).toBe(false)
  })

  it('rejects malformed cidrs at construction', () => {
    expect(() => validateCidrs(['not-a-cidr'])).toThrow()
    expect(() => validateCidrs(['192.168.1.0/99'])).toThrow()
    expect(() => validateCidrs(['192.168.1.0/24'])).not.toThrow()
  })

  it('normalizes mapped addresses', () => {
    expect(normalizeRemoteAddress('::ffff:192.168.0.1')).toBe('192.168.0.1')
    expect(normalizeRemoteAddress('192.168.0.1')).toBe('192.168.0.1')
  })
})
