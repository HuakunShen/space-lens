import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import {
  CHAT_TIMEOUT_MS,
  MAX_ERROR_BODY_CHARS,
  MAX_REQUEST_BODY_BYTES,
  MAX_RESPONSE_BODY_BYTES,
  appendCapped,
  buildChatBody,
  chatEndpoint,
  encodeChatBody,
  readCappedBody,
  redact,
  truncate,
} from '../src/ai.ts'
import { AuthService } from '../src/auth.ts'
import { buildApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import { ScanManager } from '../src/scan-store.ts'
import { startServe, type RunningServer } from '../src/server.ts'
import type { TrashPort } from '../src/trash.ts'

const KEY = 'sk-test-secret-value'

function fakeTrash(): TrashPort {
  return { trash: async (paths) => new Map(paths.map((path) => [path, null])) }
}

// ------------------------------------------------------------------ fake model

interface CapturedRequest {
  headers: http.IncomingHttpHeaders
  body: string
  url: string
}

interface FakeReply {
  status?: number
  body?: string | Buffer
  headers?: Record<string, string>
}

interface FakeModel {
  /** Base URL as an operator would paste it (no /chat/completions). */
  baseUrl: string
  requests: CapturedRequest[]
  setResponder(fn: (request: CapturedRequest) => FakeReply): void
  close(): Promise<void>
}

async function startFakeModel(
  responder: (request: CapturedRequest) => FakeReply = () => ({ status: 200, body: '{"choices":[]}' }),
): Promise<FakeModel> {
  const requests: CapturedRequest[] = []
  let current = responder
  const server = http.createServer((req, res) => {
    // A client that aborts mid-body (the 2 MiB cap) resets the socket; swallow
    // the write error so the fake upstream does not crash the test process.
    res.on('error', () => undefined)
    req.on('error', () => undefined)
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => {
      const captured: CapturedRequest = {
        headers: req.headers,
        body: Buffer.concat(chunks).toString('utf8'),
        url: req.url ?? '',
      }
      requests.push(captured)
      const reply = current(captured)
      res.writeHead(reply.status ?? 200, { 'content-type': 'application/json', ...(reply.headers ?? {}) })
      res.end(reply.body ?? '')
    })
  })
  server.on('clientError', () => undefined)
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const port = (server.address() as AddressInfo).port
  return {
    baseUrl: `http://127.0.0.1:${port}/v1`,
    requests,
    setResponder: (fn) => {
      current = fn
    },
    close: () =>
      new Promise<void>((resolve) => {
        server.close(() => resolve())
      }),
  }
}

function pairToken(server: RunningServer): Promise<string> {
  const url = new URL(server.mintPairingUrl())
  const ticket = url.searchParams.get('pair')
  if (ticket === null) throw new Error('no ticket')
  return fetch(`${server.baseUrl}/api/v1/session/exchange`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', origin: server.baseUrl },
    body: JSON.stringify({ ticket }),
  })
    .then((response) => response.json())
    .then((session) => (session as { token: string }).token)
}

// -------------------------------------------------------------- pure helpers

describe('chat endpoint guard', () => {
  it('does not suffix a pasted full endpoint twice', () => {
    expect(chatEndpoint('https://api.example.com/v1/chat/completions/')).toBe(
      'https://api.example.com/v1/chat/completions',
    )
  })

  it('builds one chat/completions endpoint from an https base URL', () => {
    expect(chatEndpoint('https://api.openai.com/v1')).toBe('https://api.openai.com/v1/chat/completions')
    // Trailing slashes and surrounding whitespace never double up the path.
    expect(chatEndpoint('  https://api.example.com/v1//  ')).toBe('https://api.example.com/v1/chat/completions')
    expect(chatEndpoint('https://api.example.com')).toBe('https://api.example.com/chat/completions')
  })

  it('allows plaintext http only for loopback', () => {
    for (const base of ['http://localhost:11434/v1', 'http://127.0.0.1:8080/v1', 'http://[::1]:8080/v1']) {
      expect(() => chatEndpoint(base)).not.toThrow()
    }
    expect(chatEndpoint('http://[::1]:8080/v1')).toBe('http://[::1]:8080/v1/chat/completions')
  })

  it('rejects remote http, userinfo and junk', () => {
    expect(() => chatEndpoint('http://api.example.com/v1')).toThrow(
      'AI base URL must use https unless the host is localhost',
    )
    // Localhost-ish names that are not loopback hosts must not pass.
    expect(() => chatEndpoint('http://localhost.example.com/v1')).toThrow()
    expect(() => chatEndpoint('https://user@example.com/v1')).toThrow('AI base URL must not contain credentials')
    expect(() => chatEndpoint('https://user:secret@example.com/v1')).toThrow()
    expect(() => chatEndpoint('file:///etc/passwd')).toThrow('AI base URL scheme is not supported: file')
    expect(() => chatEndpoint('api.openai.com/v1')).toThrow('AI base URL is not a valid URL')
    expect(() => chatEndpoint('')).toThrow('AI base URL is not set')
    expect(() => chatEndpoint('   ')).toThrow('AI base URL is not set')
  })
})

describe('chat body and size caps', () => {
  it('disables thinking only when asked', () => {
    const messages = [{ role: 'user', content: 'hi' }]
    expect(buildChatBody({ model: 'm', messages, disableThinking: false }).thinking).toBeUndefined()
    expect(buildChatBody({ model: 'm', messages, disableThinking: true }).thinking).toEqual({ type: 'disabled' })
  })

  it('carries tools only when non-empty', () => {
    const messages = [{ role: 'user', content: 'hi' }]
    const without = buildChatBody({ model: 'glm-4.6', messages, disableThinking: false })
    expect(without.model).toBe('glm-4.6')
    expect(without.messages).toEqual(messages)
    expect(without.tools).toBeUndefined()
    // An empty list is not a tools request.
    expect(buildChatBody({ model: 'm', messages, tools: [], disableThinking: false }).tools).toBeUndefined()
    const tools = [{ type: 'function' }]
    expect(buildChatBody({ model: 'm', messages, tools, disableThinking: false }).tools).toEqual(tools)
  })

  it('sets the caps the plan names', () => {
    expect(MAX_REQUEST_BODY_BYTES).toBe(1024 * 1024)
    expect(MAX_RESPONSE_BODY_BYTES).toBe(2 * 1024 * 1024)
    expect(MAX_ERROR_BODY_CHARS).toBe(300)
    expect(CHAT_TIMEOUT_MS).toBe(120_000)
  })

  it('enforces the request cap at the exact byte without echoing the body', () => {
    const small = { model: 'm', messages: [{ role: 'user', content: 'hi' }] }
    const bytes = encodeChatBody(small)
    expect(JSON.parse(new TextDecoder().decode(bytes))).toEqual(small)
    expect(bytes.length).toBeLessThanOrEqual(MAX_REQUEST_BODY_BYTES)

    const huge = { model: 'm', messages: [{ role: 'user', content: 'x'.repeat(MAX_REQUEST_BODY_BYTES) }] }
    const error = (() => {
      try {
        encodeChatBody(huge)
      } catch (caught) {
        return caught as Error
      }
      return null
    })()
    expect(error?.message).toContain('too large')
    expect(error?.message).toContain(String(MAX_REQUEST_BODY_BYTES))
    expect(error?.message).not.toContain('x'.repeat(64))
  })

  it('refuses the response chunk that would cross the cap', () => {
    const cap = 8
    let buffer: Uint8Array = new Uint8Array()
    buffer = appendCapped(buffer, new TextEncoder().encode('12345'), cap)
    // Exactly at the cap still fits.
    buffer = appendCapped(buffer, new TextEncoder().encode('678'), cap)
    expect(new TextDecoder().decode(buffer)).toBe('12345678')
    // One byte over is refused, and none of the offending chunk is buffered.
    expect(() => appendCapped(buffer, new TextEncoder().encode('9'), cap)).toThrow('too large')
    expect(new TextDecoder().decode(buffer)).toBe('12345678')
    // A single chunk larger than the whole cap never lands in memory.
    expect(() => appendCapped(new Uint8Array(), new Uint8Array(cap + 1), cap)).toThrow()
  })

  it('strips the key from messages and clips a long body', () => {
    expect(redact('rejected key sk-secret', 'sk-secret')).toBe('rejected key [redacted]')
    expect(redact('plain', null)).toBe('plain')
    expect(redact('plain', '')).toBe('plain')
    expect(truncate('x'.repeat(MAX_ERROR_BODY_CHARS + 50), MAX_ERROR_BODY_CHARS)).toHaveLength(MAX_ERROR_BODY_CHARS + 1)
    expect(truncate('short', MAX_ERROR_BODY_CHARS)).toBe('short')
  })
})

describe('readCappedBody over a real socket', () => {
  it('aborts a streamed response past the cap instead of buffering it', async () => {
    const model = await startFakeModel(() => ({
      status: 200,
      // 4 MiB against the 2 MiB cap, delivered in pieces so the reader sees
      // chunks rather than one allocation.
      body: Buffer.alloc(4 * 1024 * 1024, 'x'),
    }))
    try {
      const response = await fetch(`${model.baseUrl}/chat/completions`, { method: 'POST', body: '{}' })
      const error = await readCappedBody(response, MAX_RESPONSE_BODY_BYTES).catch((caught: unknown) => caught as Error)
      expect(error).toBeInstanceOf(Error)
      expect((error as Error).message).toContain('too large')
    } finally {
      await model.close()
    }
  })
})

// ------------------------------------------------------------- over the host

describe('ai advisor over HTTP', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-ai-host-')))
  let model: FakeModel
  let server: RunningServer
  let token = ''

  beforeAll(async () => {
    model = await startFakeModel(() => ({ status: 200, body: JSON.stringify({ id: 'cmpl_1', choices: [] }) }))
    server = await startServe({
      port: 0,
      roots: [root],
      trash: fakeTrash(),
      ai: { baseUrl: model.baseUrl, model: 'test-model', apiKey: KEY, disableThinking: true },
    })
    token = await pairToken(server)
  })

  afterAll(async () => {
    await server.stop()
    await model.close()
    rmSync(root, { recursive: true, force: true })
  })

  const api = (path: string, init?: RequestInit) =>
    fetch(`${server.baseUrl}${path}`, {
      ...init,
      headers: { authorization: `Bearer ${token}`, ...(init?.headers ?? {}) },
    })

  it('advertises the ai capability and grants the ai:chat scope', async () => {
    const capabilities = (await (await api('/api/v1/capabilities')).json()) as { ai?: boolean }
    expect(capabilities.ai).toBe(true)
    const pairing = server.mintPairingUrl()
    const ticket = new URL(pairing).searchParams.get('pair')
    const session = (await (
      await fetch(`${server.baseUrl}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: server.baseUrl },
        body: JSON.stringify({ ticket }),
      })
    ).json()) as { scopes: string[] }
    expect(session.scopes).toContain('ai:chat')
  })

  it('reports the config view without ever carrying the key', async () => {
    const response = await api('/api/v1/ai/config')
    expect(response.status).toBe(200)
    expect(await response.text()).not.toContain(KEY)
    const view = (await (await api('/api/v1/ai/config')).json()) as Record<string, unknown>
    expect(view).toEqual({
      baseUrl: model.baseUrl,
      model: 'test-model',
      hasKey: true,
      disableThinking: true,
      managed: true,
    })
  })

  it('forwards the body, the thinking flag and the bearer key', async () => {
    model.requests.length = 0
    const messages = [{ role: 'user', content: 'hello' }]
    const tools = [{ type: 'function', function: { name: 'noop' } }]
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages, tools }),
    })
    expect(response.status).toBe(200)
    expect(await response.json()).toEqual({ id: 'cmpl_1', choices: [] })
    expect(model.requests).toHaveLength(1)
    const forwarded = model.requests[0]!
    expect(forwarded.url).toBe('/v1/chat/completions')
    expect(forwarded.headers.authorization).toBe(`Bearer ${KEY}`)
    expect(forwarded.headers['content-type']).toBe('application/json')
    expect(JSON.parse(forwarded.body)).toEqual({
      model: 'test-model',
      messages,
      thinking: { type: 'disabled' },
      tools,
    })
  })

  it('omits an empty tools array', async () => {
    model.requests.length = 0
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], tools: [] }),
    })
    expect(response.status).toBe(200)
    expect(JSON.parse(model.requests[0]!.body).tools).toBeUndefined()
  })

  it('refuses an oversized request before it is sent', async () => {
    model.requests.length = 0
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'x'.repeat(MAX_REQUEST_BODY_BYTES) }] }),
    })
    expect(response.status).toBe(400)
    const problem = (await response.json()) as { problem: { code: string; message: string } }
    expect(problem.problem.code).toBe('InvalidRequest')
    expect(problem.problem.message).toContain('too large')
    expect(model.requests).toHaveLength(0)
  })

  it('aborts a response past 2 MiB with a key-free problem', async () => {
    model.setResponder(() => ({ status: 200, body: Buffer.alloc(4 * 1024 * 1024, 'x') }))
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    })
    expect(response.status).toBe(503)
    const problem = (await response.json()) as { problem: { code: string; message: string } }
    expect(problem.problem.code).toBe('Unavailable')
    expect(problem.problem.message).toContain('too large')
    expect(problem.problem.message).not.toContain(KEY)
    model.setResponder(() => ({ status: 200, body: JSON.stringify({ id: 'cmpl_1', choices: [] }) }))
  })

  it('clips an upstream error body and never leaks the key', async () => {
    model.setResponder(() => ({
      status: 500,
      body: `invalid api key ${KEY} ${'y'.repeat(1000)}`,
    }))
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    })
    expect(response.status).toBe(503)
    const problem = (await response.json()) as { problem: { code: string; message: string } }
    expect(problem.problem.code).toBe('Unavailable')
    expect(problem.problem.message).toContain('500')
    expect(problem.problem.message).toContain('[redacted]')
    expect(problem.problem.message).not.toContain(KEY)
    // The upstream body is clipped to the 300-char error budget.
    expect((problem.problem.message.match(/y/g) ?? []).length).toBeLessThanOrEqual(MAX_ERROR_BODY_CHARS)
    model.setResponder(() => ({ status: 200, body: JSON.stringify({ id: 'cmpl_1', choices: [] }) }))
  })

  it('requires a bearer token and a well-formed body', async () => {
    const unauthenticated = await fetch(`${server.baseUrl}/api/v1/ai/config`)
    expect(unauthenticated.status).toBe(401)

    for (const body of [{}, { messages: [] }, { messages: [{ role: 'user' }], extra: 1 }, { messages: 'nope' }]) {
      const response = await api('/api/v1/ai/chat', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
      })
      expect(response.status).toBe(400)
      expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('InvalidRequest')
    }
  })
})

describe('ai advisor when it is not configured', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-ai-off-')))
  let server: RunningServer
  let token = ''

  beforeAll(async () => {
    server = await startServe({ port: 0, roots: [root], trash: fakeTrash() })
    token = await pairToken(server)
  })

  afterAll(async () => {
    await server.stop()
    rmSync(root, { recursive: true, force: true })
  })

  const api = (path: string, init?: RequestInit) =>
    fetch(`${server.baseUrl}${path}`, {
      ...init,
      headers: { authorization: `Bearer ${token}`, ...(init?.headers ?? {}) },
    })

  it('advertises ai:false and withholds the ai:chat scope', async () => {
    const capabilities = (await (await api('/api/v1/capabilities')).json()) as { ai?: boolean }
    expect(capabilities.ai).toBe(false)
    const pairing = server.mintPairingUrl()
    const ticket = new URL(pairing).searchParams.get('pair')
    const session = (await (
      await fetch(`${server.baseUrl}/api/v1/session/exchange`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', origin: server.baseUrl },
        body: JSON.stringify({ ticket }),
      })
    ).json()) as { scopes: string[] }
    expect(session.scopes).not.toContain('ai:chat')
  })

  it('answers the standard Unavailable problem on both routes', async () => {
    const config = await api('/api/v1/ai/config')
    expect(config.status).toBe(503)
    expect(((await config.json()) as { problem: { code: string } }).problem.code).toBe('Unavailable')
    const chat = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    })
    expect(chat.status).toBe(503)
    expect(((await chat.json()) as { problem: { code: string } }).problem.code).toBe('Unavailable')
  })
})

describe('ai advisor scope gate', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-ai-scope-')))

  afterAll(() => {
    rmSync(root, { recursive: true, force: true })
  })

  it('refuses a session that lacks ai:chat even when ai is configured', async () => {
    const config = resolveServeConfig({
      port: 9420,
      roots: [root],
      ai: { baseUrl: 'http://127.0.0.1:9/v1', model: 'm', apiKey: null },
    })
    const auth = new AuthService({
      serviceInstanceId: 'inst_ai_scope',
      ticketTtlMs: 60_000,
      sessionTtlMs: 3_600_000,
      hostedPassword: null,
      scopes: ['scan:read'],
    })
    const events = new EventRing()
    const scans = new ScanManager({
      events,
      trash: fakeTrash(),
      roots: config.roots,
      cleanupMode: 'none',
      maxConcurrent: 2,
      maxTotal: 4,
    })
    const app = buildApp({
      config,
      auth,
      scans,
      events,
      assets: async () => new Response('no ui', { status: 404 }),
      health: {
        status: 'ok',
        serviceInstanceId: 'inst_ai_scope',
        apiMajor: API_MAJOR,
        contractVersion: CONTRACT_VERSION,
      },
      capabilities: {
        apiMajor: API_MAJOR,
        contractVersion: CONTRACT_VERSION,
        scan: { start: true, cancel: true, discovery: true, maxConcurrent: 2 },
        cleanup: { plan: false, execute: false, mode: 'none' },
        host: { folderPicker: false },
        icloud: 'unavailable',
        ai: true,
      },
      allowlist: createClientAllowlist([]),
    })
    const exchanged = auth.exchange(auth.mintTicket('', false), { origin: null })
    if (!exchanged.ok) throw new Error('exchange failed')
    const scopedToken = exchanged.session.token

    const configResponse = await app.request('http://127.0.0.1:9420/api/v1/ai/config', {
      headers: { host: '127.0.0.1:9420', authorization: `Bearer ${scopedToken}` },
    })
    expect(configResponse.status).toBe(403)
    expect(((await configResponse.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')

    const chatResponse = await app.request('http://127.0.0.1:9420/api/v1/ai/chat', {
      method: 'POST',
      headers: { host: '127.0.0.1:9420', authorization: `Bearer ${scopedToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    })
    expect(chatResponse.status).toBe(403)
    expect(((await chatResponse.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
    await scans.close()
  })
})
