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

test('app sessions restore by identity and never persist the virtual path', () => {
  const app = { appId: 'raycast', auditId: 'audit_abcdef1234567890abcdef' }
  assert.deepEqual(parseActiveScan(JSON.stringify({ scanId: 'scan_abcdefgh', app })), {
    scanId: 'scan_abcdefgh',
    app,
  })
  // Neither input form present: nothing to restore.
  assert.equal(parseActiveScan(JSON.stringify({ scanId: 'scan_abcdefgh' })), null)
  // Malformed app identity is ignored like any malformed state.
  assert.equal(
    parseActiveScan(JSON.stringify({ scanId: 'scan_abcdefgh', app: { appId: '', auditId: 'audit_x' } })),
    null,
  )
  // Legacy path payloads still parse unchanged.
  assert.deepEqual(parseActiveScan(JSON.stringify({ scanId: 'scan_abcdefgh', paths: ['/tmp'] })), {
    scanId: 'scan_abcdefgh',
    paths: ['/tmp'],
  })
})
