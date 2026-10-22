import { test } from 'node:test'
import assert from 'node:assert/strict'
import { TABLE_COLUMNS, clampWidth, defaultWidths, resolveWidths } from '../src/lib/memory/columns.ts'

test('stored widths are clamped to each column range and unknown keys ignored', () => {
  const widths = resolveWidths(TABLE_COLUMNS, { name: 9999, cpu: 10, memory: 120.6, bogus: 5, pid: 'wide' })
  assert.equal(widths.name, TABLE_COLUMNS.name.max)
  assert.equal(widths.cpu, TABLE_COLUMNS.cpu.min)
  assert.equal(widths.memory, 121)
  assert.equal(widths.pid, TABLE_COLUMNS.pid.defaultWidth)
  assert.equal('bogus' in widths, false)
})

test('missing or corrupt storage yields the defaults', () => {
  assert.deepEqual(resolveWidths(TABLE_COLUMNS, null), defaultWidths(TABLE_COLUMNS))
  assert.deepEqual(resolveWidths(TABLE_COLUMNS, 'nope'), defaultWidths(TABLE_COLUMNS))
  assert.equal(clampWidth(TABLE_COLUMNS.kind, Number.NaN), TABLE_COLUMNS.kind.defaultWidth)
})

test('the name column no longer swallows the spare width by default', () => {
  // Numbers sit within ~300 px of the names, as in Activity Monitor.
  assert.ok(TABLE_COLUMNS.name.defaultWidth <= 320)
})
