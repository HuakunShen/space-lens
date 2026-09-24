import { EventEnvelopeSchema, type EventEnvelope } from '@space-lens/contract'

export interface EventStreamOptions {
  baseUrl: string
  getToken: () => string | null
  /** Replay from this sequence; 0 replays the whole retained ring. */
  since?: number
  onEvent: (envelope: EventEnvelope) => void
  onGap?: (fromSequence: number, toSequence: number) => void
  onState: (state: 'connecting' | 'live' | 'reconnecting' | 'closed') => void
  fetchImpl?: typeof fetch
}

const MIN_BACKOFF_MS = 1_000
const MAX_BACKOFF_MS = 15_000

function parseFrames(chunk: string): { event: string; data: string }[] {
  const frames: { event: string; data: string }[] = []
  for (const block of chunk.split('\n\n')) {
    let event = 'message'
    let data = ''
    let hasData = false
    for (const line of block.split('\n')) {
      if (line.startsWith(':')) continue
      if (line.startsWith('event:')) event = line.slice(6).trim()
      else if (line.startsWith('data:')) {
        data += line.slice(5).trim()
        hasData = true
      }
    }
    if (hasData) frames.push({ event, data })
  }
  return frames
}

/**
 * SSE over fetch — not EventSource — because EventSource cannot send an
 * Authorization header. Replays from `since`, advances the cursor from the
 * envelope, and stops permanently on 401/403: a dead session needs a human to
 * pair again, not a retry loop.
 */
export function connectEventStream(options: EventStreamOptions): { close: () => void } {
  const doFetch = options.fetchImpl ?? fetch
  const base = options.baseUrl.replace(/\/$/, '')
  const controller = new AbortController()
  let cursor = options.since ?? 0
  let closed = false
  let attempts = 0
  let retryTimer: ReturnType<typeof setTimeout> | null = null

  const loop = async (): Promise<void> => {
    while (!closed) {
      options.onState(attempts === 0 ? 'connecting' : 'reconnecting')
      try {
        const token = options.getToken()
        const response = await doFetch(`${base}/api/v1/events?since=${cursor}`, {
          headers: token === null ? {} : { authorization: `Bearer ${token}` },
          signal: controller.signal,
        })
        if (response.status === 401 || response.status === 403) {
          closed = true
          options.onState('closed')
          return
        }
        if (!response.ok || response.body === null) throw new Error(`event stream status ${response.status}`)
        options.onState('live')
        attempts = 0
        const reader = response.body.getReader()
        const decoder = new TextDecoder()
        let buffer = ''
        for (;;) {
          const { done, value } = await reader.read()
          if (done) break
          buffer += decoder.decode(value, { stream: true })
          const lastBreak = buffer.lastIndexOf('\n\n')
          if (lastBreak === -1) continue
          const ready = buffer.slice(0, lastBreak + 2)
          buffer = buffer.slice(lastBreak + 2)
          for (const frame of parseFrames(ready)) {
            if (frame.event === 'hello') continue
            let parsed: unknown
            try {
              parsed = JSON.parse(frame.data)
            } catch {
              continue
            }
            const validated = EventEnvelopeSchema.safeParse(parsed)
            if (!validated.success) continue
            cursor = validated.data.sequence
            if (validated.data.payload.kind === 'eventGap') {
              options.onGap?.(validated.data.payload.fromSequence, validated.data.payload.toSequence)
            }
            options.onEvent(validated.data)
          }
        }
        // the server closed the stream cleanly: reconnect from the cursor
      } catch (error) {
        if (closed || (error instanceof Error && error.name === 'AbortError')) return
      }
      attempts += 1
      const delay = Math.min(MIN_BACKOFF_MS * 2 ** attempts, MAX_BACKOFF_MS)
      await new Promise<void>((resolve) => {
        retryTimer = setTimeout(resolve, delay)
      })
    }
  }

  void loop()
  return {
    close() {
      closed = true
      if (retryTimer !== null) clearTimeout(retryTimer)
      controller.abort()
    },
  }
}
