import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  aiAvailable,
  appScanAvailable,
  cleanupAvailable,
  discoveryAvailable,
  gitignoredAvailable,
  appAuditAvailable,
  pathRevealAvailable,
} from '../src/lib/gates.ts'
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

test('app storage view requires the appAudit capability and service method', () => {
  const auditCapabilities = capabilities({ appAudit: true })
  const auditService = { appAudit: () => Promise.reject(new Error('unused')) }
  assert.equal(appAuditAvailable({ capabilities: auditCapabilities, service: auditService }), true)
  // capability missing -> hidden
  assert.equal(
    appAuditAvailable({ capabilities: capabilities({ appAudit: undefined }), service: auditService }),
    false,
  )
  assert.equal(appAuditAvailable({ capabilities: null, service: auditService }), false)
  // service method missing (e.g. the browser HTTP host in v1) -> hidden
  assert.equal(appAuditAvailable({ capabilities: auditCapabilities, service: {} }), false)
  assert.equal(appAuditAvailable({ capabilities: auditCapabilities, service: null }), false)
})

test('app scans need the host flag and the submit arm', () => {
  const scanCapabilities = capabilities({ appScan: true })
  const scanService = { startAppScan: () => Promise.reject(new Error('unused')) }
  assert.equal(appScanAvailable({ capabilities: scanCapabilities, service: scanService }), true)
  // capability missing -> hidden
  assert.equal(
    appScanAvailable({ capabilities: capabilities({ appScan: undefined }), service: scanService }),
    false,
  )
  assert.equal(
    appScanAvailable({ capabilities: capabilities({ appScan: false }), service: scanService }),
    false,
  )
  assert.equal(appScanAvailable({ capabilities: null, service: scanService }), false)
  // service method missing (older adapters, HTTP this round) -> hidden
  assert.equal(appScanAvailable({ capabilities: scanCapabilities, service: {} }), false)
  assert.equal(appScanAvailable({ capabilities: scanCapabilities, service: null }), false)
})

test('opening a scanned path in the file manager needs the host flag and the request method', () => {
  const revealCapabilities: Capabilities = {
    ...capabilities(),
    host: { folderPicker: false, pathReveal: true, platform: 'macos' },
  }
  const revealService = { revealPath: () => Promise.reject(new Error('unused')) }
  assert.equal(pathRevealAvailable({ capabilities: revealCapabilities, service: revealService }), true)
  // host flag missing (older host) -> hidden
  assert.equal(pathRevealAvailable({ capabilities: capabilities(), service: revealService }), false)
  assert.equal(pathRevealAvailable({ capabilities: null, service: revealService }), false)
  // service method missing -> hidden
  assert.equal(pathRevealAvailable({ capabilities: revealCapabilities, service: {} }), false)
  assert.equal(pathRevealAvailable({ capabilities: revealCapabilities, service: null }), false)
})

test('the AI advisor needs the desktop proxy or an HTTP host that serves it', () => {
  const ports = { invoke: () => Promise.reject(new Error('unused')) }
  // Desktop shell: its own BYOK key store plus the invoke proxy.
  assert.equal(aiAvailable({ desktop: true, ports, hostAi: false }), true)
  // HTTP host advertising `capabilities.ai` + `aiChat` -> available in the browser.
  assert.equal(aiAvailable({ desktop: false, ports: null, hostAi: true }), true)
  assert.equal(aiAvailable({ desktop: true, ports, hostAi: true }), true)
  // Neither: a plain browser build with no host advisor stays hidden.
  assert.equal(aiAvailable({ desktop: false, ports: null, hostAi: false }), false)
  // Desktop but the ports have not loaded yet (or lack invoke), no host advisor -> hidden.
  assert.equal(aiAvailable({ desktop: true, ports: null, hostAi: false }), false)
  assert.equal(aiAvailable({ desktop: true, ports: {}, hostAi: false }), false)
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
