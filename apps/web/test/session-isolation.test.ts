import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  clearStashedCollectors,
  dropCollector,
  restoreCollector,
  shouldApplyPlan,
  stashCollector,
  stashedCollectorCount,
} from '../src/lib/session-isolation.ts'
import type { CollectorEntry } from '@space-lens/contract'

const entry = (scanId: string, path: string): CollectorEntry => ({
  id: `${scanId}-${path}`,
  scanId,
  nodeId: path,
  path,
  name: path.split('/').at(-1) ?? path,
  size: 10,
  addedAt: '2026-10-13T00:00:00.000Z',
})

test('switching sessions stashes the outgoing collector and restores the incoming one', () => {
  clearStashedCollectors()
  stashCollector('scan_aaaaaaaa', [entry('scan_aaaaaaaa', '/a/one')])
  // A scan with nothing stashed starts empty.
  assert.deepEqual(restoreCollector('scan_bbbbbbbb'), [])
  stashCollector('scan_bbbbbbbb', [entry('scan_bbbbbbbb', '/b/one'), entry('scan_bbbbbbbb', '/b/two')])
  assert.equal(stashedCollectorCount(), 2)
  assert.deepEqual(
    restoreCollector('scan_aaaaaaaa').map((item) => item.path),
    ['/a/one'],
  )
  assert.deepEqual(
    restoreCollector('scan_bbbbbbbb').map((item) => item.path),
    ['/b/one', '/b/two'],
  )
  // Restores are copies: mutating one does not corrupt the stash.
  const restored = restoreCollector('scan_aaaaaaaa')
  restored.push(entry('scan_aaaaaaaa', '/a/two'))
  assert.deepEqual(
    restoreCollector('scan_aaaaaaaa').map((item) => item.path),
    ['/a/one'],
  )
  dropCollector('scan_aaaaaaaa')
  assert.deepEqual(restoreCollector('scan_aaaaaaaa'), [])
  clearStashedCollectors()
  assert.equal(stashedCollectorCount(), 0)
})

test('late planning responses are discarded when the sequence or scan moved on', () => {
  const current = { sequence: 3, activeScanId: 'scan_aaaaaaaa' }
  assert.equal(shouldApplyPlan({ request: 3, ...current, scanId: 'scan_aaaaaaaa' }), true)
  // A newer plan started after this request.
  assert.equal(shouldApplyPlan({ request: 2, ...current, scanId: 'scan_aaaaaaaa' }), false)
  // The user switched sessions while the host verified fingerprints.
  assert.equal(shouldApplyPlan({ request: 3, ...current, scanId: 'scan_bbbbbbbb' }), false)
  assert.equal(shouldApplyPlan({ request: 3, sequence: 3, activeScanId: null, scanId: 'scan_aaaaaaaa' }), false)
})
