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

test('Linux desktop selects GNOME while browsers and Android keep Web', () => {
  assert.equal(resolveStyle('auto', true, 'Mozilla/5.0 (X11; Linux x86_64)'), 'linux')
  assert.equal(resolveStyle('auto', false, 'Mozilla/5.0 (X11; Linux x86_64)'), 'web')
  assert.equal(resolveStyle('auto', true, 'Mozilla/5.0 (Linux; Android 14)'), 'web')
  assert.equal(resolveStyle('linux', false, 'Windows NT'), 'linux')
})
test('GNOME preference survives a reload', () => {
  assert.deepEqual(parseAppearance('{"style":"linux","density":"comfortable"}'), {
    style: 'linux',
    density: 'comfortable',
  })
})
