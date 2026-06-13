import assert from 'node:assert/strict'
import test from 'node:test'

import { normalizeCliConfig, parsePresetList, parseSortMode } from './cli-config.js'

test('normalizeCliConfig defaults to current directory, all presets, and size sorting', () => {
  const options = normalizeCliConfig({
    paths: [],
    presets: [],
    ignoreHidden: false,
    sort: 'size',
  })

  assert.deepEqual(options.paths, ['.'])
  assert.deepEqual(options.presets, [])
  assert.equal(options.ignoreHidden, false)
  assert.equal(options.sort, 'size')
})

test('normalizeCliConfig supports paths, repeated presets, comma presets, hidden files, and sorting', () => {
  const options = normalizeCliConfig({
    paths: ['/repo/a', '/repo/b'],
    presets: ['node,rust', 'gitignored'],
    ignoreHidden: true,
    sort: 'path',
  })

  assert.deepEqual(options.paths, ['/repo/a', '/repo/b'])
  assert.deepEqual(options.presets, ['node', 'rust', 'gitignored'])
  assert.equal(options.ignoreHidden, true)
  assert.equal(options.sort, 'path')
})

test('parsePresetList rejects invalid presets', () => {
  assert.throws(() => parsePresetList(['python']), /Unknown preset "python"/)
})

test('parseSortMode rejects invalid sort modes', () => {
  assert.throws(() => parseSortMode('mtime'), /Unknown sort mode "mtime"/)
})
