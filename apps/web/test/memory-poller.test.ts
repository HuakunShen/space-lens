// Unit tests for the Memory view poller (T20, §6.6 of the memory-cleaner plan):
// one pull at a time, next pull scheduled `interval − elapsed`, sequence drops,
// error backoff 1 → 2 → 5 → 10 s, and pause while the tab is hidden. Every
// effect (timers, clock, visibility) is injected, so this runs headless.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { MemorySnapshot } from '@space-lens/contract'
import { BACKOFF_MS, createPoller, type PollerDeps } from '../src/lib/memory/poller.ts'

/** A structurally valid snapshot; only `sequence` matters to the poller. */
function snapshot(sequence: number): MemorySnapshot {
  return {
    sequence,
    sampledAt: '2026-10-18T00:00:00.000Z',
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 0,
      usedBytes: 0,
      freeBytes: 0,
      availableBytes: null,
      appBytes: null,
      wiredBytes: null,
      compressedBytes: null,
      cachedFilesBytes: null,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: 50 },
    groups: [],
    processes: [],
    notes: [],
  }
}

interface Timer {
  handle: number
  fn: () => void
  ms: number
}

interface Harness {
  poller: ReturnType<typeof createPoller>
  snapshots: MemorySnapshot[]
  errors: unknown[]
  readonly fetchCalls: number
  readonly timers: Timer[]
  readonly visibilityCb: (() => void) | null
  setHidden(value: boolean): void
  advance(ms: number): void
  resolve(s: MemorySnapshot): void
  reject(e: unknown): void
  lastDelay(): number | undefined
  runTimer(): void
  visibility(): void
}

/** A fake scheduler/clock/visibility environment the poller is driven through. */
function harness(): Harness {
  let now = 0
  let hidden = false
  let nextHandle = 1
  let visibilityCb: (() => void) | null = null
  let fetchCalls = 0
  const timers: Timer[] = []
  const snapshots: MemorySnapshot[] = []
  const errors: unknown[] = []
  const pending: Array<{ resolve: (s: MemorySnapshot) => void; reject: (e: unknown) => void }> = []

  const deps: PollerDeps = {
    fetch() {
      fetchCalls += 1
      return new Promise<MemorySnapshot>((resolve, reject) => pending.push({ resolve, reject }))
    },
    setTimer(fn, ms) {
      const handle = nextHandle
      nextHandle += 1
      timers.push({ handle, fn, ms })
      return handle
    },
    clearTimer(handle) {
      const index = timers.findIndex((timer) => timer.handle === handle)
      if (index >= 0) timers.splice(index, 1)
    },
    now: () => now,
    isHidden: () => hidden,
    onVisibilityChange(cb) {
      visibilityCb = cb
      return () => {
        if (visibilityCb === cb) visibilityCb = null
      }
    },
  }

  const poller = createPoller(deps, {
    intervalMs: 1000,
    onSnapshot: (s) => snapshots.push(s),
    onError: (e) => errors.push(e),
  })

  return {
    poller,
    snapshots,
    errors,
    get fetchCalls() {
      return fetchCalls
    },
    get timers() {
      return timers
    },
    get visibilityCb() {
      return visibilityCb
    },
    setHidden(value) {
      hidden = value
    },
    advance(ms) {
      now += ms
    },
    resolve(s) {
      pending.shift()?.resolve(s)
    },
    reject(e) {
      pending.shift()?.reject(e)
    },
    lastDelay() {
      return timers.at(-1)?.ms
    },
    runTimer() {
      const timer = timers.shift()
      if (!timer) throw new Error('no scheduled timer')
      now += timer.ms
      timer.fn()
    },
    visibility() {
      visibilityCb?.()
    },
  }
}

/** Drain microtasks so an injected-timer pull can settle. */
const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve))

test('start pulls at once and schedules the next at interval minus elapsed', async () => {
  const h = harness()
  h.poller.start()
  assert.equal(h.fetchCalls, 1)
  h.advance(120)
  h.resolve(snapshot(1))
  await flush()
  assert.deepEqual(
    h.snapshots.map((s) => s.sequence),
    [1],
  )
  assert.equal(h.lastDelay(), 880)

  h.runTimer()
  assert.equal(h.fetchCalls, 2)
  h.resolve(snapshot(2))
  await flush()
  assert.equal(h.lastDelay(), 1000)
})

test('never overlaps pulls: a reply must land before the next fetch', async () => {
  const h = harness()
  h.poller.start()
  assert.equal(h.fetchCalls, 1)
  // Visibility churn while a pull is in flight must not start a second one.
  h.setHidden(true)
  h.visibility()
  h.setHidden(false)
  h.visibility()
  assert.equal(h.fetchCalls, 1)
  assert.equal(h.timers.length, 0)
  h.resolve(snapshot(1))
  await flush()
  assert.equal(h.fetchCalls, 1)
  assert.equal(h.lastDelay(), 1000)
})

test('an error backs off 1s, 2s, 5s, 10s, then stays at 10s', async () => {
  const h = harness()
  h.poller.start()
  for (const expected of BACKOFF_MS) {
    h.reject(new Error('sampler unavailable'))
    await flush()
    assert.equal(h.lastDelay(), expected)
    if (expected !== BACKOFF_MS[BACKOFF_MS.length - 1]) h.runTimer()
  }
  assert.equal(h.errors.length, BACKOFF_MS.length)
  // A further failure keeps the 10s ceiling.
  h.runTimer()
  h.reject(new Error('still down'))
  await flush()
  assert.equal(h.lastDelay(), 10000)
})

test('a snapshot that is not newer than the last applied one is dropped', async () => {
  const h = harness()
  h.poller.start()
  h.resolve(snapshot(5))
  await flush()
  assert.deepEqual(
    h.snapshots.map((s) => s.sequence),
    [5],
  )

  h.runTimer()
  h.resolve(snapshot(5))
  await flush()
  h.runTimer()
  h.resolve(snapshot(3))
  await flush()
  assert.deepEqual(
    h.snapshots.map((s) => s.sequence),
    [5],
  )

  h.runTimer()
  h.resolve(snapshot(6))
  await flush()
  assert.deepEqual(
    h.snapshots.map((s) => s.sequence),
    [5, 6],
  )
})

test('polling pauses while hidden and resumes when shown again', async () => {
  const h = harness()
  h.setHidden(true)
  h.poller.start()
  assert.equal(h.fetchCalls, 0)
  assert.equal(h.timers.length, 0)

  h.setHidden(false)
  h.visibility()
  assert.equal(h.fetchCalls, 1)
  h.resolve(snapshot(1))
  await flush()
  assert.equal(h.lastDelay(), 1000)

  // A pending timer is cancelled the moment the tab hides.
  h.setHidden(true)
  h.visibility()
  assert.equal(h.timers.length, 0)
  h.setHidden(false)
  h.visibility()
  assert.equal(h.fetchCalls, 2)
})

test('setInterval changes the delay of subsequent pulls', async () => {
  const h = harness()
  h.poller.start()
  h.resolve(snapshot(1))
  await flush()
  assert.equal(h.lastDelay(), 1000)

  h.poller.setInterval(5000)
  h.runTimer()
  h.resolve(snapshot(2))
  await flush()
  assert.equal(h.lastDelay(), 5000)
})

test('stop cancels the timer, unsubscribes and ignores the in-flight reply', async () => {
  const h = harness()
  h.poller.start()
  h.poller.stop()
  assert.equal(h.visibilityCb, null)
  h.resolve(snapshot(1))
  await flush()
  assert.deepEqual(h.snapshots, [])
  assert.equal(h.timers.length, 0)
  h.visibility()
  assert.equal(h.fetchCalls, 1)
})
