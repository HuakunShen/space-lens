import { EventEnvelopeSchema, type EventEnvelope, type EventPayload } from '@space-lens/contract'

const DEFAULT_MAX_EVENTS = 1024
const DEFAULT_MAX_BYTES = 1024 * 1024

export interface ReplayResult {
  events: EventEnvelope[]
  gap: { fromSequence: number; toSequence: number } | null
}

type Listener = (envelope: EventEnvelope) => void

/**
 * Bounded in-memory event log. Events are invalidation hints, never the source
 * of truth: a payload that fails contract validation is dropped (and counted),
 * not thrown — one bad publish must not kill the host.
 */
export class EventRing {
  private events: EventEnvelope[] = []
  private bytes = 0
  private sequence = 0
  private dropped = 0
  private readonly listeners = new Set<Listener>()

  private readonly maxEvents: number
  private readonly maxBytes: number

  constructor(maxEvents: number = DEFAULT_MAX_EVENTS, maxBytes: number = DEFAULT_MAX_BYTES) {
    this.maxEvents = maxEvents
    this.maxBytes = maxBytes
  }

  publish(payload: EventPayload): EventEnvelope | null {
    const candidate = {
      sequence: this.sequence + 1,
      emittedAt: new Date().toISOString(),
      payload,
    }
    const parsed = EventEnvelopeSchema.safeParse(candidate)
    if (!parsed.success) {
      this.dropped += 1
      return null
    }
    const envelope = parsed.data
    this.sequence = envelope.sequence
    this.events.push(envelope)
    this.bytes += JSON.stringify(envelope).length
    while (this.bytes > this.maxBytes && this.events.length > 1) {
      const oldest = this.events.shift()
      if (oldest !== undefined) this.bytes -= JSON.stringify(oldest).length
    }
    while (this.events.length > this.maxEvents) {
      const oldest = this.events.shift()
      if (oldest !== undefined) this.bytes -= JSON.stringify(oldest).length
    }
    for (const listener of this.listeners) {
      try {
        listener(envelope)
      } catch {
        // a broken subscriber must not affect the ring or other subscribers
      }
    }
    return envelope
  }

  replay(since: number): ReplayResult {
    const events = this.events.filter((envelope) => envelope.sequence > since)
    let gap: { fromSequence: number; toSequence: number } | null = null
    if (since > 0 && this.events.length > 0) {
      const oldest = this.events[0].sequence
      if (since < oldest - 1) gap = { fromSequence: since + 1, toSequence: oldest - 1 }
    }
    return { events, gap }
  }

  subscribe(listener: Listener): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  get highWatermark(): number {
    return this.sequence
  }

  get droppedCount(): number {
    return this.dropped
  }
}
