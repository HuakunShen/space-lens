import { describe, expect, it } from 'vitest'

import { ServiceError, createHttpService } from '../src/http.ts'

function jsonResponse(payload: unknown, status: number): Response {
  return new Response(JSON.stringify(payload), { status, headers: { 'content-type': 'application/json' } })
}

function streamResponse(chunks: Uint8Array[]): Response {
  return new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        for (const chunk of chunks) controller.enqueue(chunk)
        controller.close()
      },
    }),
    { status: 200, headers: { 'content-type': 'text/event-stream' } },
  )
}

describe('http service ai streaming', () => {
  it('posts stream:true with the bearer token and hands each chunk to the callback', async () => {
    const encoder = new TextEncoder()
    const sse = 'data: {"choices":[{"delta":{"content":"hi"}}]}\n\ndata: [DONE]\n\n'
    const fetchImpl = (async (input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe('http://localhost/api/v1/ai/chat')
      expect(init?.method).toBe('POST')
      expect((init?.headers as Record<string, string>).authorization).toBe('Bearer session')
      expect(JSON.parse(init?.body as string)).toEqual({
        messages: [{ role: 'user', content: 'hi' }],
        stream: true,
      })
      return streamResponse([encoder.encode(sse.slice(0, 12)), encoder.encode(sse.slice(12))])
    }) as typeof fetch

    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    const chunks: string[] = []
    await service.aiChatStream({ messages: [{ role: 'user', content: 'hi' }] }, (text) => chunks.push(text))
    expect(chunks.join('')).toBe(sse)
  })

  it('reassembles a multi-byte character split across chunk boundaries', async () => {
    const encoder = new TextEncoder()
    const bytes = encoder.encode('a😀b')
    const fetchImpl = (async () => streamResponse([bytes.subarray(0, 2), bytes.subarray(2)])) as typeof fetch

    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => 'session', fetchImpl })
    const chunks: string[] = []
    await service.aiChatStream({ messages: [{ role: 'user', content: 'hi' }] }, (text) => chunks.push(text))
    expect(chunks.join('')).toBe('a😀b')
    expect(chunks.every((chunk) => !chunk.includes('\uFFFD'))).toBe(true)
  })

  it('throws the same ServiceError as request on a non-ok response', async () => {
    const fetchImpl = (async () =>
      jsonResponse({ problem: { code: 'Forbidden', message: 'read-only host', retryable: false } }, 403)) as typeof fetch

    const service = createHttpService({ baseUrl: 'http://localhost', getToken: () => null, fetchImpl })
    const error = await service
      .aiChatStream({ messages: [{ role: 'user', content: 'hi' }] }, () => undefined)
      .catch((caught: unknown) => caught)
    expect(error).toBeInstanceOf(ServiceError)
    expect((error as ServiceError).code).toBe('Forbidden')
    expect((error as ServiceError).status).toBe(403)
  })
})
