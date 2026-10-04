import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parseActiveScan } from '../src/lib/resume.ts'

test('only a valid bounded scan reference can resume; malformed browser state is ignored', () => {
  assert.equal(parseActiveScan(null), null)
  assert.equal(parseActiveScan('{'), null)
  assert.equal(parseActiveScan(JSON.stringify({ scanId: '/Users/hk', paths: ['/'] })), null)
  assert.equal(parseActiveScan(JSON.stringify({ scanId: 'scan_12345678', paths: [] })), null)
  assert.equal(parseActiveScan(JSON.stringify({ scanId: 'scan_12345678', paths: Array(17).fill('/') })), null)
  assert.deepEqual(parseActiveScan(JSON.stringify({ scanId: 'scan_12345678', paths: ['/', '/System/Volumes/Data'], focusNodeId: 'abcd' })), { scanId: 'scan_12345678', paths: ['/', '/System/Volumes/Data'], focusNodeId: 'abcd' })
})
