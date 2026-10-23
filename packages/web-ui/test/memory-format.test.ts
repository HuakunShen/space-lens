// Unit tests for the memory view's pure number formatting (T17).
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { formatBytes, formatPercentValue, formatShare } from '../src/lib/memory/format.ts'

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
