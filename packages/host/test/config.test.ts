import { describe, expect, it } from 'vitest'

import { ConfigError, resolveBindHost, resolveServeConfig, HOSTED_PASSWORD_ENV, AI_API_KEY_ENV } from '../src/config.ts'

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

describe('ai advisor config', () => {
  it('is off unless an ai block is given', () => {
    expect(resolveServeConfig({}).ai).toBeNull()
  })

  it('validates the base URL and model at startup', () => {
    for (const ai of [
      { baseUrl: 'http://api.example.com/v1', model: 'm' },
      { baseUrl: '', model: 'm' },
      { baseUrl: 'https://api.example.com/v1', model: '' },
      { baseUrl: 'https://user@example.com/v1', model: 'm' },
      { baseUrl: 'file:///etc/passwd', model: 'm' },
    ]) {
      expect(() => resolveServeConfig({ ai })).toThrow(ConfigError)
    }
    const resolved = resolveServeConfig({ ai: { baseUrl: 'https://api.example.com/v1/', model: ' glm-4.6 ' } })
    expect(resolved.ai).toEqual({
      endpoint: 'https://api.example.com/v1/chat/completions',
      // The reported base URL keeps the operator's trailing slash (like the
      // desktop config view); the endpoint is the normalized target.
      baseUrl: 'https://api.example.com/v1/',
      model: 'glm-4.6',
      apiKey: null,
      disableThinking: false,
    })
  })

  it('keeps the key host-side, falling back to the environment', () => {
    const previous = process.env[AI_API_KEY_ENV]
    delete process.env[AI_API_KEY_ENV]
    try {
      expect(
        resolveServeConfig({ ai: { baseUrl: 'https://x.example/v1', model: 'm', apiKey: 'sk-option' } }).ai?.apiKey,
      ).toBe('sk-option')
      expect(resolveServeConfig({ ai: { baseUrl: 'https://x.example/v1', model: 'm' } }).ai?.apiKey).toBeNull()
      process.env[AI_API_KEY_ENV] = 'sk-env'
      expect(resolveServeConfig({ ai: { baseUrl: 'https://x.example/v1', model: 'm' } }).ai?.apiKey).toBe('sk-env')
      // An explicit null means "no key" even when the environment has one.
      expect(
        resolveServeConfig({ ai: { baseUrl: 'https://x.example/v1', model: 'm', apiKey: null } }).ai?.apiKey,
      ).toBeNull()
    } finally {
      if (previous === undefined) delete process.env[AI_API_KEY_ENV]
      else process.env[AI_API_KEY_ENV] = previous
    }
  })
})

describe('memory and process-control config', () => {
  it('refuses process control without the read-only memory view', () => {
    expect(() => resolveServeConfig({ allowProcessControl: true })).toThrow(ConfigError)
    expect(() => resolveServeConfig({ allowMemory: true })).not.toThrow()
    expect(() => resolveServeConfig({ allowProcessControl: true, allowMemory: true })).not.toThrow()
  })

  it('requires a loopback bind for process control', () => {
    expect(() =>
      resolveServeConfig({ host: '0.0.0.0', allowLan: true, allowMemory: true, allowProcessControl: true }),
    ).toThrow(ConfigError)
  })

  it('requires loopback-only UI origins for process control', () => {
    const previous = process.env[HOSTED_PASSWORD_ENV]
    process.env[HOSTED_PASSWORD_ENV] = 'a-long-enough-password'
    try {
      expect(() =>
        resolveServeConfig({
          allowMemory: true,
          allowProcessControl: true,
          uiOrigins: ['https://spacelens.example.workers.dev'],
        }),
      ).toThrow(ConfigError)
      expect(() =>
        resolveServeConfig({ allowMemory: true, allowProcessControl: true, uiOrigins: ['http://127.0.0.1:5173'] }),
      ).not.toThrow()
    } finally {
      if (previous === undefined) delete process.env[HOSTED_PASSWORD_ENV]
      else process.env[HOSTED_PASSWORD_ENV] = previous
    }
  })

  it('resolves the flags, defaulting both off', () => {
    expect(resolveServeConfig({})).toMatchObject({ allowMemory: false, allowProcessControl: false })
    expect(resolveServeConfig({ allowMemory: true })).toMatchObject({ allowMemory: true, allowProcessControl: false })
    expect(resolveServeConfig({ allowMemory: true, allowProcessControl: true })).toMatchObject({
      allowMemory: true,
      allowProcessControl: true,
    })
  })
})
