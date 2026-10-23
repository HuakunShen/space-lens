import assert from 'node:assert/strict'
import { test } from 'node:test'
import { glassStrength, parseAppearance, resolveStyle } from '../src/lib/appearance.ts'
test('browser keeps web style while native hosts default to their platform', () => {
  assert.equal(resolveStyle('auto', false, 'Macintosh'), 'web')
  assert.equal(resolveStyle('auto', true, 'Macintosh'), 'macos')
  assert.equal(resolveStyle('auto', true, 'Windows NT'), 'windows')
  assert.equal(resolveStyle('windows', false, 'Macintosh'), 'windows')
})
test('corrupt or old stored preferences recover to defaults', () => {
  assert.deepEqual(parseAppearance('not json'), {
    style: 'auto',
    density: 'compact',
    transparency: 33,
    accent: 'system',
  })
  assert.deepEqual(parseAppearance('{"style":"broken","density":"broken","transparency":"broken","accent":"broken"}'), {
    style: 'auto',
    density: 'compact',
    transparency: 33,
    accent: 'system',
  })
  assert.deepEqual(
    parseAppearance('{"style":"macos","density":"comfortable","transparency":"glassy","accent":"default"}'),
    {
      style: 'macos',
      density: 'comfortable',
      transparency: 60,
      accent: 'default',
    },
  )
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
    transparency: 33,
    accent: 'system',
  })
})

test('transparency parses to a clamped percentage; old names still load', () => {
  assert.equal(parseAppearance('{"transparency":"solid"}').transparency, 0)
  assert.equal(parseAppearance('{"transparency":"balanced"}').transparency, 33)
  assert.equal(parseAppearance('{"transparency":"glassy"}').transparency, 60)
  assert.equal(parseAppearance('{"transparency":72.4}').transparency, 72)
  assert.equal(parseAppearance('{"transparency":250}').transparency, 100)
  assert.equal(parseAppearance('{"transparency":-5}').transparency, 0)
  assert.equal(parseAppearance('{"transparency":"frosted"}').transparency, 33)
  assert.equal(parseAppearance('{"style":"web","density":"compact"}').transparency, 33)
  assert.equal(parseAppearance(null).transparency, 33)
})

test('glass strength is about 1 at the default and opaque at 0', () => {
  assert.equal(glassStrength(33), 1.003)
  assert.ok(glassStrength(0) > 1.25)
  assert.equal(glassStrength(100), 0.4)
})

test('a stored appearance round-trips through JSON', () => {
  for (const transparency of [0, 33, 60, 100]) {
    const stored = JSON.stringify({ style: 'macos', density: 'comfortable', transparency })
    assert.deepEqual(parseAppearance(stored), {
      style: 'macos',
      density: 'comfortable',
      transparency,
      accent: 'system',
    })
  }
})

test('accent parses strictly and defaults to system', () => {
  assert.equal(parseAppearance('{"accent":"default"}').accent, 'default')
  assert.equal(parseAppearance('{"accent":"system"}').accent, 'system')
  // unknown values and legacy payloads without the key fall back to system
  assert.equal(parseAppearance('{"accent":"hotdog"}').accent, 'system')
  assert.equal(parseAppearance('{"style":"web"}').accent, 'system')
  assert.equal(parseAppearance(null).accent, 'system')
})
