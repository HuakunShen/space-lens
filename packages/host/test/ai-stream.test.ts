import { mkdtempSync, realpathSync, rmSync } from 'node:fs'
import http from 'node:http'
import type { AddressInfo } from 'node:net'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterAll, beforeAll, describe, expect, it } from 'vitest'

import { API_MAJOR, CONTRACT_VERSION } from '@space-lens/contract'
import { MAX_REQUEST_BODY_BYTES, MAX_STREAM_BODY_BYTES, forwardChatStream } from '../src/ai.ts'
import { AuthService } from '../src/auth.ts'
import { buildApp } from '../src/app.ts'
import { createClientAllowlist } from '../src/cidr.ts'
import { resolveServeConfig } from '../src/config.ts'
import { EventRing } from '../src/events.ts'
import { ProblemError } from '../src/problems.ts'
import { ScanManager } from '../src/scan-store.ts'
import { startServe, type RunningServer } from '../src/server.ts'
import type { TrashPort } from '../src/trash.ts'

const KEY = 'sk-test-secret-value'
const ENCODER = new TextEncoder()

function fakeTrash(): TrashPort {
  return { trash: async (paths) => new Map(paths.map((path) => [path, null])) }
}

/** Reads a forwarded stream to completion, rejecting where the source errors. */
async function drain(stream: ReadableStream<Uint8Array>): Promise<Uint8Array> {
  const reader = stream.getReader()
  const parts: Uint8Array[] = []
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    if (value !== undefined) parts.push(value)
  }
  const total = parts.reduce((sum, part) => sum + part.length, 0)
  const merged = new Uint8Array(total)
  let offset = 0
  for (const part of parts) {
    merged.set(part, offset)
    offset += part.length
  }
  return merged
}

const CHAT = {
  endpoint: 'https://api.example.com/v1/chat/completions',
  model: 'test-model',
  disableThinking: false,
  messages: [{ role: 'user', content: 'hi' }],
} as const

// ------------------------------------------------------- forwardChatStream unit

describe('forwardChatStream', () => {
  const SSE = [
    'data: {"choices":[{"delta":{"content":"Hel"}}]}\n\n',
    'data: {"choices":[{"delta":{"content":"lo"}}]}\n\n',
    'data: [DONE]\n\n',
  ]

  it('forwards raw SSE chunks unchanged and adds stream:true to the request', async () => {
    const seen: { url: string; init: RequestInit }[] = []
    const fetchImpl = (async (url: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
      seen.push({ url: String(url), init: init ?? {} })
      return new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            for (const chunk of SSE) controller.enqueue(ENCODER.encode(chunk))
            controller.close()
          },
        }),
        { status: 200, headers: { 'content-type': 'text/event-stream' } },
      )
    }) as typeof fetch

    const stream = await forwardChatStream({ ...CHAT, apiKey: KEY, fetchImpl })
    expect(new TextDecoder().decode(await drain(stream))).toBe(SSE.join(''))

    expect(seen).toHaveLength(1)
    expect(seen[0]!.url).toBe(CHAT.endpoint)
    expect((seen[0]!.init.headers as Record<string, string>).authorization).toBe(`Bearer ${KEY}`)
    expect(JSON.parse(new TextDecoder().decode(seen[0]!.init.body as Uint8Array))).toEqual({
      model: 'test-model',
      messages: CHAT.messages,
      stream: true,
    })
  })

  it('throws the redacted problem on a non-ok upstream and forwards no chunk', async () => {
    const fetchImpl = (async () =>
      new Response(`invalid api key ${KEY} ${'y'.repeat(1000)}`, { status: 500 })) as typeof fetch
    const error = await forwardChatStream({ ...CHAT, apiKey: KEY, fetchImpl }).catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ProblemError)
    expect((error as ProblemError).problem.code).toBe('Unavailable')
    const message = (error as Error).message
    expect(message).toContain('500')
    expect(message).toContain('[redacted]')
    expect(message).not.toContain(KEY)
    expect((message.match(/y/g) ?? []).length).toBeLessThanOrEqual(300)
  })

  it('errors the stream once forwarded bytes would cross 4 MiB', async () => {
    const megabyte = new Uint8Array(1024 * 1024).fill(0x78)
    const fetchImpl = (async () =>
      new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            for (let index = 0; index < 5; index += 1) controller.enqueue(megabyte)
            controller.close()
          },
        }),
        { status: 200 },
      )) as typeof fetch

    const stream = await forwardChatStream({ ...CHAT, apiKey: KEY, fetchImpl })
    const reader = stream.getReader()
    let forwarded = 0
    const error = await (async () => {
      for (;;) {
        try {
          const { done, value } = await reader.read()
          if (done) return null
          if (value !== undefined) forwarded += value.length
        } catch (caught) {
          return caught as Error
        }
      }
    })()
    expect(error).toBeInstanceOf(Error)
    expect(error?.message).toContain('too large')
    expect(error?.message).toContain(String(MAX_STREAM_BODY_BYTES))
    // The crossing chunk is refused: exactly the cap is forwarded, no more.
    expect(forwarded).toBe(MAX_STREAM_BODY_BYTES)
  })

  it('refuses an oversized request before fetch is ever called', async () => {
    let called = false
    const fetchImpl = (async () => {
      called = true
      return new Response('')
    }) as typeof fetch
    const error = await forwardChatStream({
      ...CHAT,
      apiKey: KEY,
      messages: [{ role: 'user', content: 'x'.repeat(MAX_REQUEST_BODY_BYTES) }],
      fetchImpl,
    }).catch((caught: unknown) => caught)
    expect(called).toBe(false)
    expect(error).toBeInstanceOf(ProblemError)
    expect((error as ProblemError).problem.code).toBe('InvalidRequest')
    expect((error as Error).message).toContain('too large')
  })

  it('cancelling the returned stream cancels the upstream reader', async () => {
    let cancelled = false
    const fetchImpl = (async () =>
      new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(ENCODER.encode('data: one\n\n'))
          },
          cancel() {
            cancelled = true
          },
        }),
        { status: 200 },
      )) as typeof fetch

    const stream = await forwardChatStream({ ...CHAT, apiKey: KEY, fetchImpl })
    const reader = stream.getReader()
    await reader.read()
    await reader.cancel()
    expect(cancelled).toBe(true)
  })

  it('combines the caller signal so aborting it aborts the upstream request', async () => {
    let seenSignal: AbortSignal | undefined
    const fetchImpl = (async (_url: Parameters<typeof fetch>[0], init?: Parameters<typeof fetch>[1]) => {
      seenSignal = init?.signal ?? undefined
      return new Response(
        new ReadableStream<Uint8Array>({
          start(controller) {
            controller.enqueue(ENCODER.encode('data: one\n\n'))
          },
        }),
        { status: 200 },
      )
    }) as typeof fetch

    const controller = new AbortController()
    const stream = await forwardChatStream({ ...CHAT, apiKey: KEY, fetchImpl, signal: controller.signal })
    expect(seenSignal?.aborted).toBe(false)
    controller.abort()
    expect(seenSignal?.aborted).toBe(true)
    await stream.cancel().catch(() => undefined)
  })
})

// ------------------------------------------------------------- over the host

interface Captured {
  headers: http.IncomingHttpHeaders
  body: string
  url: string
}

type Responder = (request: Captured) => { status?: number; body: string }

interface FakeStreamModel {
  baseUrl: string
  requests: Captured[]
  setResponder(fn: Responder): void
  close(): Promise<void>
}

async function startFakeStreamModel(responder: Responder): Promise<FakeStreamModel> {
  const requests: Captured[] = []
  let current = responder
  const server = http.createServer((req, res) => {
    res.on('error', () => undefined)
    req.on('error', () => undefined)
    const chunks: Buffer[] = []
    req.on('data', (chunk: Buffer) => chunks.push(chunk))
    req.on('end', () => {
      const captured: Captured = {
        headers: req.headers,
        body: Buffer.concat(chunks).toString('utf8'),
        url: req.url ?? '',
      }
      requests.push(captured)
      const reply = current(captured)
      const payload = Buffer.from(reply.body, 'utf8')
      res.writeHead(reply.status ?? 200, { 'content-type': 'text/event-stream' })
      // Two writes so the host sees more than one wire chunk.
      const midpoint = Math.floor(payload.length / 2)
      res.write(payload.subarray(0, midpoint))
      res.write(payload.subarray(midpoint))
      res.end()
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
  const ticket = new URL(server.mintPairingUrl()).searchParams.get('pair')
  if (ticket === null) throw new Error('no ticket')
  return fetch(`${server.baseUrl}/api/v1/session/exchange`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', origin: server.baseUrl },
    body: JSON.stringify({ ticket }),
  })
    .then((response) => response.json())
    .then((session) => (session as { token: string }).token)
}

describe('ai stream over HTTP', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-ai-stream-')))
  const SSE = 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: [DONE]\n\n'
  let model: FakeStreamModel
  let server: RunningServer
  let token = ''

  beforeAll(async () => {
    model = await startFakeStreamModel(() => ({ status: 200, body: SSE }))
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

  it('answers 200 text/event-stream and forwards the raw SSE bytes', async () => {
    model.requests.length = 0
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], stream: true }),
    })
    expect(response.status).toBe(200)
    expect(response.headers.get('content-type')).toBe('text/event-stream')
    expect(response.headers.get('cache-control')).toBe('no-store')
    expect(response.headers.get('x-accel-buffering')).toBe('no')
    // The SSE bytes reach the client untouched (the host never parses them).
    expect(await response.text()).toBe(SSE)
    expect(model.requests).toHaveLength(1)
    expect(JSON.parse(model.requests[0]!.body).stream).toBe(true)
  })

  it('keeps the non-streaming response path off the streaming branch', async () => {
    model.requests.length = 0
    model.setResponder(() => ({ status: 200, body: JSON.stringify({ id: 'cmpl_1', choices: [] }) }))
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }] }),
    })
    expect(response.status).toBe(200)
    expect((response.headers.get('content-type') ?? '').startsWith('application/json')).toBe(true)
    expect(await response.json()).toEqual({ id: 'cmpl_1', choices: [] })
    expect(JSON.parse(model.requests[0]!.body).stream).toBeUndefined()
    model.setResponder(() => ({ status: 200, body: SSE }))
  })

  it('answers a redacted problem JSON (no stream) for a non-ok upstream', async () => {
    model.setResponder(() => ({ status: 500, body: `invalid api key ${KEY}` }))
    const response = await api('/api/v1/ai/chat', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], stream: true }),
    })
    expect(response.status).toBe(503)
    expect((response.headers.get('content-type') ?? '').startsWith('application/json')).toBe(true)
    const problem = (await response.json()) as { problem: { code: string; message: string } }
    expect(problem.problem.code).toBe('Unavailable')
    expect(problem.problem.message).toContain('[redacted]')
    expect(problem.problem.message).not.toContain(KEY)
    model.setResponder(() => ({ status: 200, body: SSE }))
  })
})

describe('ai stream scope gate', () => {
  const root = realpathSync(mkdtempSync(join(tmpdir(), 'spacelens-ai-stream-scope-')))

  afterAll(() => {
    rmSync(root, { recursive: true, force: true })
  })

  it('refuses a session without ai:chat with the usual 403', async () => {
    const config = resolveServeConfig({
      port: 9420,
      roots: [root],
      ai: { baseUrl: 'http://127.0.0.1:9/v1', model: 'm', apiKey: null },
    })
    const auth = new AuthService({
      serviceInstanceId: 'inst_ai_stream_scope',
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
        serviceInstanceId: 'inst_ai_stream_scope',
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

    const response = await app.request('http://127.0.0.1:9420/api/v1/ai/chat', {
      method: 'POST',
      headers: { host: '127.0.0.1:9420', authorization: `Bearer ${scopedToken}`, 'content-type': 'application/json' },
      body: JSON.stringify({ messages: [{ role: 'user', content: 'hi' }], stream: true }),
    })
    expect(response.status).toBe(403)
    expect(((await response.json()) as { problem: { code: string } }).problem.code).toBe('Forbidden')
    await scans.close()
  })
})
