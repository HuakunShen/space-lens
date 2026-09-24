import { describe, expect, it } from 'vitest'

import { ConfigError, resolveBindHost, resolveServeConfig, HOSTED_PASSWORD_ENV } from '../src/config.ts'

describe('bind host resolution', () => {
  it('defaults to loopback', () => {
    expect(resolveBindHost(undefined)).toMatchObject({ address: '127.0.0.1', loopback: true })
    expect(resolveBindHost('localhost')).toMatchObject({ address: '127.0.0.1', loopback: true })
    expect(resolveBindHost('::1')).toMatchObject({ loopback: true })
  })

  it('recognizes all-interface binds', () => {
    expect(resolveBindHost('0.0.0.0')).toMatchObject({ address: '0.0.0.0', loopback: false })
    expect(resolveBindHost('::')).toMatchObject({ address: '::', loopback: false })
    expect(resolveBindHost('*')).toMatchObject({ loopback: false })
  })

  it('accepts explicit addresses and rejects nonsense', () => {
    expect(resolveBindHost('192.168.50.7')).toMatchObject({ address: '192.168.50.7', loopback: false })
    expect(() => resolveBindHost('en0-nope')).toThrow(ConfigError)
  })
})

describe('serve config guard rules', () => {
  it('refuses non-loopback binds without a client restriction', () => {
    expect(() => resolveServeConfig({ host: '0.0.0.0' })).toThrow(ConfigError)
    expect(() => resolveServeConfig({ host: '0.0.0.0', allowLan: true })).not.toThrow()
    expect(() => resolveServeConfig({ host: '0.0.0.0', allowCidr: ['192.168.0.0/16'] })).not.toThrow()
    expect(() => resolveServeConfig({ host: 'loopback' })).not.toThrow()
  })

  it('requires the hosted password env for non-loopback UI origins', () => {
    const previous = process.env[HOSTED_PASSWORD_ENV]
    delete process.env[HOSTED_PASSWORD_ENV]
    try {
      expect(() => resolveServeConfig({ uiOrigins: ['https://spacelens.example.workers.dev'] })).toThrow(ConfigError)
      expect(() => resolveServeConfig({ uiOrigins: ['http://127.0.0.1:5173'] })).not.toThrow()
      process.env[HOSTED_PASSWORD_ENV] = 'a-long-enough-password'
      expect(() => resolveServeConfig({ uiOrigins: ['https://spacelens.example.workers.dev'] })).not.toThrow()
    } finally {
      if (previous === undefined) delete process.env[HOSTED_PASSWORD_ENV]
      else process.env[HOSTED_PASSWORD_ENV] = previous
    }
  })

  it('rejects sloppy origins and ports', () => {
    expect(() => resolveServeConfig({ uiOrigins: ['https://x.dev/path'] })).toThrow(ConfigError)
    expect(() => resolveServeConfig({ uiOrigins: ['*'] })).toThrow(ConfigError)
    expect(() => resolveServeConfig({ port: 70000 })).toThrow(ConfigError)
    expect(() => resolveServeConfig({ port: 0 })).not.toThrow()
  })

  it('machine mode forces loopback, port 0, json, and no origins', () => {
    const config = resolveServeConfig({ machine: true, host: '0.0.0.0', allowLan: true, uiOrigins: ['https://x.dev'] })
    expect(config).toMatchObject({ machine: true, json: true, port: 0, allowLan: false, uiOrigins: [] })
    expect(config.bind.loopback).toBe(true)
    expect(config.portExplicit).toBe(false)
  })
})
