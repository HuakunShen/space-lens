import { test } from 'node:test'
import assert from 'node:assert/strict'
import { formatNodeSize, formatNodeName } from '../src/lib/node-size.ts'
test('an unscanned zero estimate never becomes a known zero-byte item', () => {
  assert.equal(formatNodeSize({ size: 0, scanState: 'skipped' }), 'Unknown')
  assert.equal(formatNodeSize({ size: 0, scanState: 'complete' }), '0 B')
  assert.equal(formatNodeSize({ size: 65536, scanState: 'partial' }), '64.0 KiB')
  assert.equal(formatNodeSize(null), '')
})
test('binary sizes use IEC labels instead of decimal disk-capacity units', () => {
  assert.equal(formatNodeSize({ size: 1024, scanState: 'complete' }), '1.00 KiB')
  assert.equal(formatNodeSize({ size: 1024 ** 3, scanState: 'complete' }), '1.00 GiB')
})
test('APFS aliases are counted elsewhere, while inaccessible sizes remain unknown', () => {
  assert.equal(formatNodeSize({ size: 0, scanState: 'skipped', skipReason: 'duplicate-directory' }), 'Counted elsewhere')
  assert.equal(formatNodeSize({ size: 0, scanState: 'skipped', skipReason: 'permission-denied' }), 'Unknown')
})
test('filesystem roots have a useful display name even when basename is empty', () => {
  assert.equal(formatNodeName({ name: '', path: '/' }, true), 'Macintosh HD')
  assert.equal(formatNodeName({ name: '', path: '/' }), 'Filesystem')
  assert.equal(formatNodeName({ name: 'Users', path: '/Users' }, true), 'Users')
})
