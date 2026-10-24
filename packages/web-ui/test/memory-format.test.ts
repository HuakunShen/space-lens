// Unit tests for the memory view's pure number formatting (T17).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  formatBytes,
  formatDuration,
  formatPercentValue,
  formatShare,
  formatStartTime,
  runningSeconds,
  runningSecondsFromIso,
} from '../src/lib/memory/format.ts'

test('formatBytes uses binary units and drops precision as values grow', () => {
  assert.equal(formatBytes(0), '0 B')
  assert.equal(formatBytes(512), '512 B')
  assert.equal(formatBytes(1024), '1.0 KB')
  assert.equal(formatBytes(1536), '1.5 KB')
  assert.equal(formatBytes(1024 * 1024 * 757), '757 MB')
  assert.equal(formatBytes(1024 ** 3 * 48), '48.0 GB')
})

test('formatBytes is defensive about non-finite and negative input', () => {
  assert.equal(formatBytes(Number.NaN), '0 B')
  assert.equal(formatBytes(Number.POSITIVE_INFINITY), '0 B')
  assert.equal(formatBytes(-1), '0 B')
})

test('formatPercentValue keeps one decimal below ten and none above', () => {
  assert.equal(formatPercentValue(0.4), '0.4')
  assert.equal(formatPercentValue(0), '0')
  assert.equal(formatPercentValue(6), '6.0')
  assert.equal(formatPercentValue(42.7), '43')
  assert.equal(formatPercentValue(Number.NaN), '0')
})

test('formatShare renders a 0..1 share as a whole percent', () => {
  assert.equal(formatShare(0), '0%')
  assert.equal(formatShare(0.064), '6%')
  assert.equal(formatShare(0.5), '50%')
  assert.equal(formatShare(1), '100%')
})

test('formatDuration keeps the two largest units and never invents zero', () => {
  assert.equal(formatDuration(0), '0s')
  assert.equal(formatDuration(12), '12s')
  assert.equal(formatDuration(68), '1m 8s')
  assert.equal(formatDuration(3_600), '1h 0m')
  assert.equal(formatDuration(3_600 + 12 * 60), '1h 12m')
  assert.equal(formatDuration(86_400 + 4 * 3_600), '1d 4h')
  assert.equal(formatDuration(-1), '—')
  assert.equal(formatDuration(Number.NaN), '—')
})

test('formatStartTime refuses the unknown zero timestamp', () => {
  assert.equal(formatStartTime(0), null)
  assert.equal(formatStartTime(Number.NaN), null)
  assert.equal(typeof formatStartTime(1_729_180_800_000_000), 'string')
})

test('runningSeconds measures against the snapshot clock', () => {
  const sampledAt = '2026-10-18T10:05:00.000Z'
  const startTimeUs = Date.parse(sampledAt) * 1_000 - 1_074 * 1_000_000
  assert.equal(runningSeconds(startTimeUs, sampledAt), 1_074)
  assert.equal(runningSeconds(0, sampledAt), null)
  assert.equal(runningSeconds(startTimeUs, 'not a date'), null)
  const startedAt = new Date(Date.parse(sampledAt) - 3_900_000).toISOString()
  assert.equal(runningSecondsFromIso(startedAt, sampledAt), 3_900)
  assert.equal(runningSecondsFromIso('not a date', sampledAt), null)
  assert.equal(runningSecondsFromIso('1970-01-01T00:00:00.000Z', sampledAt), null)
})
