import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { ScanCoverage, StorageBreakdown } from '@space-lens/contract'
import { reconcileStorage, skipReasonRows } from '../src/lib/storage-reconcile.ts'

const GB = 1_000_000_000

const startupDisk: StorageBreakdown = {
  container: 'disk3',
  capacityBytes: 995 * GB,
  inUseBytes: 515 * GB,
  purgeableBytes: 119 * GB,
  scannedBytes: 287 * GB,
  volumes: [
    { name: 'Macintosh HD - Data', role: 'Data', device: 'disk3s5', inUseBytes: 458 * GB },
    { name: 'VM', role: 'VM', device: 'disk3s6', inUseBytes: 30 * GB },
    { name: 'Macintosh HD', role: 'System', device: 'disk3s1', inUseBytes: 14 * GB },
    { name: 'Preboot', role: 'Preboot', device: 'disk3s2', inUseBytes: 11 * GB },
    { name: 'Recovery', role: 'Recovery', device: 'disk3s3', inUseBytes: 2 * GB },
    { name: 'Update', role: 'Update', device: 'disk3s4', inUseBytes: 0 },
  ],
  snapshots: { volume: 'disk3s5', count: 25, timeMachineCount: 25, oldestTimeMachine: '2026-10-05T16:01:11' },
}

test('the startup disk splits into measured, other volumes and unseen space that add up', () => {
  const result = reconcileStorage(startupDisk)
  assert.equal(result.measured.bytes, 287 * GB)
  assert.deepEqual(
    result.otherVolumes.map((row) => [row.label, row.bytes]),
    [
      ['VM', 30 * GB],
      ['Preboot', 11 * GB],
      ['Recovery', 2 * GB],
    ],
  )
  assert.equal(result.unseen.bytes, (515 - 287 - 43) * GB)
  assert.equal(
    result.measured.bytes + result.otherVolumes.reduce((total, row) => total + row.bytes, 0) + result.unseen.bytes,
    result.inUseBytes,
  )
  assert.match(result.unseen.detail, /25 local snapshots \(oldest 2026-10-05/)
  assert.equal(result.overcounted, false)
  // Data and the sealed System volume are what a startup-disk scan walks.
  assert.deepEqual(
    result.scannedVolumes.map((row) => row.key),
    ['disk3s5', 'disk3s1'],
  )
})

test('an external APFS disk without roles treats its largest volume as the scanned one', () => {
  const result = reconcileStorage({
    ...startupDisk,
    container: 'disk9',
    inUseBytes: 100 * GB,
    scannedBytes: 90 * GB,
    volumes: [
      { name: 'Portable', role: 'None', device: 'disk9s1', inUseBytes: 95 * GB },
      { name: 'Backups', role: 'None', device: 'disk9s2', inUseBytes: 5 * GB },
    ],
    snapshots: null,
  })
  assert.deepEqual(
    result.otherVolumes.map((row) => row.label),
    ['Backups'],
  )
  assert.equal(result.unseen.bytes, 5 * GB)
  assert.doesNotMatch(result.unseen.detail, /snapshot/)
})

test('a scan that measures more than the container reports never shows negative space', () => {
  const result = reconcileStorage({ ...startupDisk, scannedBytes: 600 * GB })
  assert.equal(result.measured.bytes, 515 * GB)
  assert.equal(result.unseen.bytes, 0)
  assert.equal(result.overcounted, true)
})

test('skip reasons are labelled, sorted, and symlinks are not missing data', () => {
  const coverage = {
    skippedCount: 161_808,
    skippedByReason: {
      symlink: 150_000,
      'permission-denied': 11_000,
      'special-file': 2_500,
      'mount-boundary': 8,
      'duplicate-directory': 18,
      'odd-new-reason': 800,
    },
  } as unknown as ScanCoverage
  const rows = skipReasonRows(coverage)
  assert.deepEqual(
    rows.map((row) => [row.reason, row.missing]),
    [
      ['symlink', false],
      ['permission-denied', true],
      ['special-file', false],
      ['odd-new-reason', true],
      ['duplicate-directory', false],
      ['mount-boundary', true],
    ],
  )
  assert.equal(rows[3]?.label, 'odd-new-reason')
  assert.equal(rows[4]?.label, 'Duplicate folder aliases')
  assert.deepEqual(skipReasonRows({ skippedCount: 3 } as unknown as ScanCoverage), [])
})
