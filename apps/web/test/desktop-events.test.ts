import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  DESKTOP_EVENT_NAME,
  reduceScanEvent,
  startDesktopEventStream,
  type DesktopEventEnvelope,
} from '../src/lib/desktop-events.ts'
import type { ScanStatus } from '@space-lens/contract'

const status = (scanId: string, state: ScanStatus['state']): ScanStatus => ({
  scanId,
  state,
  message: '',
  progress: null,
  currentPath: null,
  bytesScanned: 0,
  entriesScanned: 0,
  rootIds: [],
  label: null,
  updatedAt: '2026-10-05T00:00:00.000Z',
})

const frame = (sequence: number, payload: DesktopEventEnvelope['payload']) => ({ event: { sequence, payload } })

test('frames at or below the watermark are dropped idempotently', () => {
  const first = reduceScanEvent(0, 'scan_1', {
    sequence: 1,
    payload: { kind: 'scan.updated', status: status('scan_1', 'scanning') },
  })
  assert.equal(first.status?.state, 'scanning')
  assert.equal(first.watermark, 1)
  // The same frame again (replay/live overlap or reconnect) is a no-op.
  const replay = reduceScanEvent(1, 'scan_1', {
    sequence: 1,
    payload: { kind: 'scan.updated', status: status('scan_1', 'scanning') },
  })
  assert.equal(replay.status, null)
  assert.equal(replay.ready, false)
  assert.equal(replay.watermark, 1)
})

test('scan.updated applies and scan.completed fires the ready hook exactly once', () => {
  const decision = reduceScanEvent(0, 'scan_1', {
    sequence: 2,
    payload: { kind: 'scan.completed', status: status('scan_1', 'ready') },
  })
  assert.equal(decision.status?.state, 'ready')
  assert.equal(decision.ready, true)
  // A second completed frame (or a late duplicate) does not fire again.
  const late = reduceScanEvent(decision.watermark, 'scan_1', {
    sequence: 2,
    payload: { kind: 'scan.completed', status: status('scan_1', 'ready') },
  })
  assert.equal(late.ready, false)
  assert.equal(late.status, null)
})

test('frames for other scans and unknown kinds are ignored but advance the watermark', () => {
  const other = reduceScanEvent(0, 'scan_1', {
    sequence: 3,
    payload: { kind: 'scan.updated', status: status('scan_other', 'scanning') },
  })
  assert.equal(other.status, null)
  assert.equal(other.watermark, 3)
  const unknown = reduceScanEvent(3, 'scan_1', { sequence: 4, payload: { kind: 'session' } })
  assert.equal(unknown.status, null)
  assert.equal(unknown.watermark, 4)
})

type Harness = {
  ports: {
    invoke(cmd: string, payload?: Record<string, unknown>): Promise<unknown>
    listen(event: string, handler: (event: unknown) => void): Promise<() => void>
  }
  emit(frame: unknown): void
  invokes(): { cmd: string; payload: Record<string, unknown> }[]
  unlistened(): boolean
}

function harness(): Harness {
  const calls: { cmd: string; payload: Record<string, unknown> }[] = []
  let handler: ((event: unknown) => void) | null = null
  let listened = false
  let unlistenCalled = false
  return {
    ports: {
      async invoke(cmd, payload = {}) {
        calls.push({ cmd, payload })
        if (cmd === 'sl_connect') return { sessionId: 'sess_1' }
        if (cmd === 'sl_events_subscribe') {
          assert.ok(listened, 'subscribe must happen only after listen is registered')
          assert.equal(payload.sessionId, 'sess_1')
          return { subscriptionId: 'sub_1', highWatermark: 0, replay: [], serviceInstanceId: 'inst' }
        }
        return { unsubscribed: true }
      },
      async listen(_event, received) {
        assert.equal(_event, DESKTOP_EVENT_NAME)
        listened = true
        handler = received
        return () => {
          unlistenCalled = true
          handler = null
        }
      },
    },
    emit(frame) {
      handler?.(frame)
    },
    invokes: () => calls,
    unlistened: () => unlistenCalled,
  }
}

test('stream listens before subscribing, applies frames, and close() truly unsubscribes', async () => {
  const h = harness()
  const statuses: string[] = []
  let readyCalls = 0
  let activeScanId: string | null = null
  const stream = await startDesktopEventStream(h.ports, {
    activeScanId: () => activeScanId,
    onStatus: (fresh) => statuses.push(fresh.state),
    onReady: () => {
      readyCalls += 1
    },
  })
  assert.deepEqual(
    h.invokes().map((call) => call.cmd),
    ['sl_connect', 'sl_events_subscribe'],
  )
  h.emit(frame(1, { kind: 'scan.updated', status: status('scan_1', 'scanning') }))
  assert.deepEqual(statuses, [], 'events for the inactive scan are ignored')
  activeScanId = 'scan_1'
  h.emit(frame(2, { kind: 'scan.updated', status: status('scan_1', 'scanning') }))
  h.emit(frame(3, { kind: 'scan.completed', status: status('scan_1', 'ready') }))
  // replay/live overlap: sequence 3 again through the channel
  h.emit(frame(3, { kind: 'scan.completed', status: status('scan_1', 'ready') }))
  assert.deepEqual(statuses, ['scanning', 'ready'])
  assert.equal(readyCalls, 1)
  await stream.close()
  const closeCalls = h.invokes().filter((call) => call.cmd === 'sl_events_unsubscribe')
  assert.equal(closeCalls.length, 1)
  assert.deepEqual(closeCalls[0].payload, { sessionId: 'sess_1', subscriptionId: 'sub_1' })
  assert.ok(h.unlistened(), 'close() must remove the listener after unsubscribing')
})

test('late frames after close produce no side effects', async () => {
  const h = harness()
  let statuses = 0
  const stream = await startDesktopEventStream(h.ports, {
    activeScanId: () => 'scan_1',
    onStatus: () => {
      statuses += 1
    },
    onReady: () => {},
  })
  await stream.close()
  h.emit(frame(9, { kind: 'scan.updated', status: status('scan_1', 'ready') }))
  assert.equal(statuses, 0, 'the handler is gone after close()')
})
