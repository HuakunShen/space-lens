import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseAppearance, resolveStyle } from '../src/lib/appearance.ts'
test('browser keeps web style while native hosts default to their platform', () => {
  assert.equal(resolveStyle('auto', false, 'Macintosh'), 'web')
  assert.equal(resolveStyle('auto', true, 'Macintosh'), 'macos')
  assert.equal(resolveStyle('auto', true, 'Windows NT'), 'windows')
  assert.equal(resolveStyle('windows', false, 'Macintosh'), 'windows')
})
test('corrupt or old stored preferences recover to defaults', () => {
  assert.deepEqual(parseAppearance('not json'), { style: 'auto', density: 'compact' })
  assert.deepEqual(parseAppearance('{"style":"broken","density":"broken"}'), { style: 'auto', density: 'compact' })
  assert.deepEqual(parseAppearance('{"style":"macos","density":"comfortable"}'), {
    style: 'macos',
    density: 'comfortable',
  })
})
