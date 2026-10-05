import assert from 'node:assert/strict'
import { test } from 'node:test'
import { cleanupAvailable, discoveryAvailable, gitignoredAvailable } from '../src/lib/gates.ts'
import type { Capabilities, ScanStatus } from '@space-lens/contract'

const capabilities = (
  scan: Partial<Capabilities['scan']> = {},
  cleanup: Partial<Capabilities['cleanup']> = {},
): Capabilities => ({
  apiMajor: 1,
  contractVersion: '1.0.0',
  scan: { start: true, cancel: true, discovery: true, maxConcurrent: 1, ...scan },
  cleanup: { plan: true, execute: true, mode: 'trash', ...cleanup },
  host: { folderPicker: false },
  icloud: 'unavailable',
})

const coverage = { mode: 'local-only', logicalBytes: 1 } as unknown as ScanStatus['coverage']
const service = { discover: () => Promise.reject(new Error('unused')) }

test('desktop and browser both enable the discovery views once the backend declares them', () => {
  const input = { capabilities: capabilities(), service }
  assert.equal(discoveryAvailable(input), true)
  // identical on the desktop flavor — the old hard gate on
  // desktop+coverage is gone, both hosts derive from the same report
  assert.equal(gitignoredAvailable({ ...input, coverage, scannedPaths: ['/Users/hk/project'] }), true)
})

test('discovery is unavailable without the capability or the discover method', () => {
  assert.equal(discoveryAvailable({ capabilities: null, service }), false)
  assert.equal(discoveryAvailable({ capabilities: capabilities(), service: null }), false)
  assert.equal(
    discoveryAvailable({
      capabilities: { ...capabilities(), scan: { ...capabilities().scan, discovery: undefined } },
      service,
    }),
    false,
  )
})

test('gitignored discovery stays disabled for full startup-disk scans only', () => {
  const input = { capabilities: capabilities(), service }
  assert.equal(gitignoredAvailable({ ...input, coverage, scannedPaths: ['/'] }), false)
  assert.equal(gitignoredAvailable({ ...input, coverage, scannedPaths: ['/Users/hk', '/System/Volumes/Data'] }), false)
  // no coverage (legacy scan) or ordinary paths keep it enabled
  assert.equal(gitignoredAvailable({ ...input, coverage: undefined, scannedPaths: ['/'] }), true)
  assert.equal(gitignoredAvailable({ ...input, coverage, scannedPaths: ['/Volumes/Ext'] }), true)
  // and it inherits the discovery gate
  assert.equal(
    gitignoredAvailable({
      capabilities: capabilities({ discovery: false }),
      service,
      coverage,
      scannedPaths: ['/Volumes/Ext'],
    }),
    false,
  )
})

test('cleanup stays desktop-only for covered scans and capability-gated everywhere', () => {
  assert.equal(cleanupAvailable({ capabilities: capabilities(), desktop: true, coverage }), true)
  assert.equal(cleanupAvailable({ capabilities: capabilities(), desktop: false, coverage }), false)
  // without coverage both flavors behave the same
  assert.equal(cleanupAvailable({ capabilities: capabilities(), desktop: false, coverage: undefined }), true)
  assert.equal(cleanupAvailable({ capabilities: capabilities(), desktop: true, coverage: undefined }), true)
  // no execute capability → never available
  assert.equal(
    cleanupAvailable({
      capabilities: capabilities({}, { execute: false, plan: false, mode: 'none' }),
      desktop: true,
      coverage,
    }),
    false,
  )
  assert.equal(cleanupAvailable({ capabilities: null, desktop: true, coverage }), false)
})
