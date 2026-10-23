/**
 * Memory snapshot poller for the Memory view (T20, §6.6 of the memory-cleaner
 * plan).
 *
 * One pull at a time: the next pull is scheduled `intervalMs − elapsed` after
 * the previous reply, so a slow host throttles itself instead of queueing
 * requests. A reply whose `sequence` is not newer than the last applied one is
 * dropped. While the tab is hidden polling stops and resumes on the next
 * visibility change. Errors back off 1 → 2 → 5 → 10 s, then hold at 10 s.
 *
 * Every effect (timers, wall clock, visibility) is injected, so the whole
 * machine is exercised by `node --test` without a browser or real time.
 */
import type { MemorySnapshot } from '@space-lens/contract'

export interface PollerDeps {
  /** One snapshot read; resolves with the parsed reply, rejects on transport error. */
  fetch: () => Promise<MemorySnapshot>
  /** Schedule `fn` in `ms`; the return value is an opaque handle. */
  setTimer(fn: () => void, ms: number): unknown
  /** Cancel a handle returned by `setTimer`. */
  clearTimer(handle: unknown): void
  /** Monotonic-enough wall clock in milliseconds. */
  now(): number
  /** Whether the document is currently hidden. */
  isHidden(): boolean
  /** Subscribe to visibility changes; the returned function unsubscribes. */
  onVisibilityChange(cb: () => void): () => void
}

export interface PollerOptions {
  /** Pull interval in milliseconds (1000 / 2000 / 5000 from the toolbar). */
  intervalMs: number
  onSnapshot(s: MemorySnapshot): void
  onError(e: unknown): void
}

export interface Poller {
  start(): void
  stop(): void
  /** Change the interval used for subsequent pulls; the pending timer is untouched. */
  setInterval(ms: number): void
}

/** §6.6 error backoff, in milliseconds; the last value is the ceiling. */
export const BACKOFF_MS = [1000, 2000, 5000, 10000] as const

export function createPoller(deps: PollerDeps, opts: PollerOptions): Poller {
  let intervalMs = opts.intervalMs
  let started = false
  let timer: unknown = null
  let lastSequence = -1
  let backoff = -1
  let inFlight = false
  let unsubscribe: (() => void) | null = null
  /** Bumped on start/stop so a reply from a previous run is ignored. */
  let generation = 0

  function cancelTimer(): void {
    if (timer !== null) {
      deps.clearTimer(timer)
      timer = null
    }
  }

  function schedule(ms: number): void {
    if (!started) return
    timer = deps.setTimer(() => {
      timer = null
      void pull()
    }, Math.max(0, Math.round(ms)))
  }

  async function pull(): Promise<void> {
    if (!started || inFlight || deps.isHidden()) return
    const gen = generation
    inFlight = true
    const startedAt = deps.now()
    try {
      const next = await deps.fetch()
      if (gen !== generation || !started) return
      const elapsed = deps.now() - startedAt
      if (next.sequence > lastSequence) {
        lastSequence = next.sequence
        opts.onSnapshot(next)
      }
      backoff = -1
      if (!deps.isHidden()) schedule(intervalMs - elapsed)
    } catch (error) {
      if (gen !== generation || !started) return
      opts.onError(error)
      backoff = Math.min(backoff + 1, BACKOFF_MS.length - 1)
      if (!deps.isHidden()) schedule(BACKOFF_MS[backoff] ?? BACKOFF_MS[BACKOFF_MS.length - 1])
    } finally {
      if (gen === generation) inFlight = false
    }
  }

  function onVisibility(): void {
    if (!started) return
    if (deps.isHidden()) {
      cancelTimer()
      return
    }
    if (timer === null && !inFlight) void pull()
  }

  return {
    start(): void {
      if (started) return
      started = true
      generation += 1
      inFlight = false
      unsubscribe = deps.onVisibilityChange(onVisibility)
      if (!deps.isHidden()) void pull()
    },
    stop(): void {
      if (!started) return
      started = false
      generation += 1
      inFlight = false
      cancelTimer()
      unsubscribe?.()
      unsubscribe = null
    },
    setInterval(ms: number): void {
      intervalMs = ms
    },
  }
}
