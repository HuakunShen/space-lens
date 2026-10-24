// The Memory sidebar's section grammar is a closed set: a stored value that is
// not one of the three sections must fall back to Overview, never crash.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import { MEMORY_SECTIONS, MEMORY_SECTION_LABELS, parseMemorySection } from '../src/lib/memory/sections.ts'

test('every section has a label', () => {
  assert.deepEqual(Object.keys(MEMORY_SECTION_LABELS), [...MEMORY_SECTIONS])
})

test('parseMemorySection accepts only the known sections', () => {
  assert.equal(parseMemorySection('processes'), 'processes')
  assert.equal(parseMemorySection('stale'), 'stale')
  assert.equal(parseMemorySection('activity'), 'activity')
  assert.equal(parseMemorySection('overview'), 'overview')
  assert.equal(parseMemorySection('nope'), 'overview')
  assert.equal(parseMemorySection(null), 'overview')
})
