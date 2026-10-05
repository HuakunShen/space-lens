/**
 * Desktop live-event consumption. The Tauri imports stay in
 * `tauri-ports.ts`; everything here is pure logic or operates on the ports
 * passed in, so the browser bundle can share the reducer with the SSE path.
 *
 * Ordering contract with the backend (commands.rs `sl_events_subscribe`): the
 * listen is registered BEFORE the subscribe, so the synchronous replay cannot
 * outrun the handler; replay and the forwarded live channel overlap by
 * design, and the sequence watermark below deduplicates that overlap.
 */
import type { ScanStatus } from '@space-lens/contract'

/** Must match EVENT_NAME in apps/desktop/src-tauri/src/commands.rs. */
export const DESKTOP_EVENT_NAME = 'spacelens://event'

export interface DesktopEventEnvelope {
  sequence: number
  payload?: { kind?: string; status?: ScanStatus }
}

export interface ScanEventDecision {
  /** Watermark after applying the frame; it advances even for ignored frames. */
  watermark: number
  /** The status to apply, or null when the frame is dropped. */
  status: ScanStatus | null
  /** The active scan reached `ready` — run the ready side effects once. */
  ready: boolean
}

/**
 * The single decision function for one scan-event envelope, shared with the
 * browser SSE onEvent (workbench.svelte.ts): only scan.updated/scan.completed
 * frames of the active scan apply; stale, late, or replay-overlap frames
 * (sequence ≤ watermark) are dropped idempotently.
 */
export function reduceScanEvent(
  watermark: number,
  activeScanId: string | null,
  envelope: DesktopEventEnvelope,
): ScanEventDecision {
  if (envelope.sequence <= watermark) return { watermark, status: null, ready: false }
  const ignored: ScanEventDecision = { watermark: envelope.sequence, status: null, ready: false }
  const kind = envelope.payload?.kind
  if (kind !== 'scan.updated' && kind !== 'scan.completed') return ignored
  const status = envelope.payload?.status
  if (!status || status.scanId !== activeScanId) return ignored
  return { watermark: envelope.sequence, status, ready: status.state === 'ready' }
}

export interface DesktopEventHooks {
  /** Read on every frame: the active scan can change while the stream is open. */
  activeScanId(): string | null
  onStatus(status: ScanStatus): void
  onReady(): void
}

export interface DesktopEventStreamHandle {
  /** Truly unsubscribes backend-side before removing the listener. */
  close(): Promise<void>
}

/**
 * Starts the desktop push stream: listen first, then subscribe with the
 * backend's synchronous replay, then let the backend forwarding thread deliver
 * live frames over the same event. The reducer's watermark makes the overlap
 * between replay and channel delivery harmless.
 */
export async function startDesktopEventStream(
  ports: {
    invoke(cmd: string, payload?: Record<string, unknown>): Promise<unknown>
    listen?(event: string, handler: (event: unknown) => void): Promise<() => void>
  },
  hooks: DesktopEventHooks,
): Promise<DesktopEventStreamHandle> {
  if (typeof ports.listen !== 'function') {
    throw new Error('event push requires the tauri listen port; polling remains the fallback')
  }
  // sl_connect is idempotent per window: it returns the existing session
  // instead of allocating a second engine.
  const metadata = (await ports.invoke('sl_connect', {})) as { sessionId?: string }
  const sessionId = metadata.sessionId
  let watermark = 0
  const unlisten = await ports.listen(DESKTOP_EVENT_NAME, (raw) => {
    const frame = raw as { event?: DesktopEventEnvelope }
    if (!frame?.event) return
    const decision = reduceScanEvent(watermark, hooks.activeScanId(), frame.event)
    watermark = decision.watermark
    if (decision.status) hooks.onStatus(decision.status)
    if (decision.ready) hooks.onReady()
  })
  let subscriptionId = ''
  try {
    const ack = (await ports.invoke('sl_events_subscribe', { sessionId, afterSequence: 0 })) as {
      subscriptionId?: string
    }
    subscriptionId = ack.subscriptionId ?? ''
  } catch (error) {
    // A failed subscribe leaves no backend subscription; do not leak the
    // just-registered listener either.
    unlisten()
    throw error
  }
  return {
    async close() {
      try {
        // Removing the subscription sender ends the backend forwarding thread;
        // the unlisten only stops our own handler.
        await ports.invoke('sl_events_unsubscribe', { sessionId, subscriptionId })
      } catch {
        // The backend also self-cleans when the window (or session) is gone.
      }
      unlisten()
    },
  }
}
