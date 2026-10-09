import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  CHART_MODES,
  DEFAULT_CHART_MODE,
  DEFAULT_TREEMAP_DENSITY,
  parseChartMode,
  parseTreemapDensity,
} from '../src/lib/chart-mode.ts'

test('every family id round-trips', () => {
  for (const mode of CHART_MODES) assert.equal(parseChartMode(mode), mode)
})

test('nothing stored, or nonsense stored, lands on the sunburst', () => {
  assert.equal(parseChartMode(null), DEFAULT_CHART_MODE)
  assert.equal(parseChartMode(''), DEFAULT_CHART_MODE)
  assert.equal(parseChartMode('pie'), DEFAULT_CHART_MODE)
  assert.equal(parseChartMode('SUNBURST'), DEFAULT_CHART_MODE)
})

test('pre-treemap installs keep the view they last chose', () => {
  // `flat` and `nested` were modes in their own right before the treemap
  // gained a density axis; `tree` was an early spelling of nested.
  for (const legacy of ['flat', 'nested', 'tree']) {
    assert.equal(parseChartMode(legacy), 'treemap', `${legacy} upscales to the treemap family`)
  }
  assert.equal(parseTreemapDensity(null, 'flat'), 'flat')
  assert.equal(parseTreemapDensity(null, 'nested'), 'nested')
  assert.equal(parseTreemapDensity(null, 'tree'), 'nested')
})

test('an explicit density outranks the legacy mode key', () => {
  assert.equal(parseTreemapDensity('flat', 'nested'), 'flat')
  assert.equal(parseTreemapDensity('nested', 'flat'), 'nested')
})

test('a missing or unusable density falls back to the default', () => {
  assert.equal(parseTreemapDensity(null), DEFAULT_TREEMAP_DENSITY)
  assert.equal(parseTreemapDensity('sideways'), DEFAULT_TREEMAP_DENSITY)
  assert.equal(parseTreemapDensity(null, 'sunburst'), DEFAULT_TREEMAP_DENSITY)
})
