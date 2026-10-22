import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { ScanVolume } from '@space-lens/contract'
import { estimateScanProgress, formatDuration, scanBaseline } from '../src/lib/scan-progress.ts'

const GiB = 1024 ** 3
const volumes: ScanVolume[] = [
  { path: '/', totalBytes: 926 * GiB, availableBytes: 452 * GiB, freeBytes: 452 * GiB, isLocal: true },
  {
    path: '/Volumes/Portable2TB',
    totalBytes: 1900 * GiB,
    availableBytes: 330 * GiB,
    freeBytes: 330 * GiB,
    isLocal: true,
  },
]

test('the last scan of the same location is the preferred baseline', () => {
  const history = { 'Macintosh HD': { bytes: 267 * GiB, entries: 6_292_277, at: '2026-10-22T21:10:00Z' } }
  assert.deepEqual(scanBaseline('Macintosh HD', ['/', '/System/Volumes/Data'], history, volumes), {
    bytes: 267 * GiB,
    entries: 6_292_277,
    source: 'last-scan',
  })
})

test('a whole-volume scan without history falls back to used space', () => {
  assert.deepEqual(scanBaseline('Macintosh HD', ['/', '/System/Volumes/Data'], {}, volumes), {
    bytes: 474 * GiB,
    entries: null,
    source: 'disk-used',
  })
  assert.equal(scanBaseline('/Volumes/Portable2TB/', ['/Volumes/Portable2TB/'], {}, volumes)?.bytes, 1570 * GiB)
})

test('a folder without history has no baseline', () => {
  assert.equal(scanBaseline('/Users/hk/Library/Caches', ['/Users/hk/Library/Caches'], {}, volumes), null)
})

test('progress averages bytes and items and projects the remaining time', () => {
  const estimate = estimateScanProgress({
    bytesScanned: 100 * GiB,
    entriesScanned: 3_000_000,
    elapsedMs: 30_000,
    baseline: { bytes: 200 * GiB, entries: 6_000_000, source: 'last-scan' },
  })
  assert.equal(estimate.fraction, 0.5)
  assert.equal(estimate.etaMs, 30_000)
  assert.equal(estimate.finishing, false)
  assert.equal(Math.round((estimate.bytesPerSecond ?? 0) / GiB), 3)
})

test('the bar never reaches 100% before the host finishes', () => {
  const estimate = estimateScanProgress({
    bytesScanned: 210 * GiB,
    entriesScanned: 0,
    elapsedMs: 60_000,
    baseline: { bytes: 200 * GiB, entries: null, source: 'disk-used' },
  })
  assert.equal(estimate.fraction, 0.99)
  assert.equal(estimate.finishing, true)
  assert.equal(estimate.etaMs, null)
})

test('no ETA until enough of the scan has run, and none without a baseline', () => {
  const baseline = { bytes: 200 * GiB, entries: null, source: 'disk-used' as const }
  assert.equal(estimateScanProgress({ bytesScanned: GiB, entriesScanned: 0, elapsedMs: 10_000, baseline }).etaMs, null)
  assert.equal(
    estimateScanProgress({ bytesScanned: 50 * GiB, entriesScanned: 0, elapsedMs: 1_000, baseline }).etaMs,
    null,
  )
  const none = estimateScanProgress({ bytesScanned: GiB, entriesScanned: 9, elapsedMs: 1_000, baseline: null })
  assert.equal(none.fraction, null)
  assert.equal(none.bytesPerSecond, GiB)
})

test('durations read naturally', () => {
  assert.equal(formatDuration(45_000), '45 s')
  assert.equal(formatDuration(150_000), '3 min')
  assert.equal(formatDuration(3_900_000), '1 h 5 min')
  assert.equal(formatDuration(7_200_000), '2 h')
})
