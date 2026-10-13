import assert from 'node:assert/strict'
import { test } from 'node:test'
import {
  forgetRecentAppScan,
  isRecentAppStale,
  loadRecentAppScans,
  markRecentAppStale,
  rememberRecentAppScan,
} from '../src/lib/recent-apps.ts'

interface MemoryStorage {
  store: Record<string, string>
}

function installStorage(): MemoryStorage {
  const memory: MemoryStorage = { store: {} }
  const storage = {
    getItem: (key: string) => memory.store[key] ?? null,
    setItem: (key: string, value: string) => {
      memory.store[key] = value
    },
    removeItem: (key: string) => {
      delete memory.store[key]
    },
  }
  ;(globalThis as unknown as { window: unknown }).window = { localStorage: storage }
  return memory
}

const AUDIT = 'audit_abcdef1234567890abcdef'

test('recent app scans persist the pinned shape with provenance and cap at five', () => {
  installStorage()
  assert.deepEqual(loadRecentAppScans(), [])
  rememberRecentAppScan({
    appId: 'raycast',
    appName: 'Raycast',
    auditId: AUDIT,
    generatedAt: '2026-10-13T00:00:00.000Z',
  })
  const [entry] = loadRecentAppScans()
  assert.equal(entry?.appId, 'raycast')
  assert.equal(entry?.auditId, AUDIT)
  assert.equal(entry?.generatedAt, '2026-10-13T00:00:00.000Z')
  assert.ok(entry?.lastScannedAt)
  // Re-scanning bumps the entry to the front without duplicating it.
  rememberRecentAppScan({ appId: 'codex', appName: 'Codex CLI', auditId: AUDIT })
  rememberRecentAppScan({ appId: 'raycast', appName: 'Raycast', auditId: AUDIT })
  assert.deepEqual(
    loadRecentAppScans().map((item) => item.appId),
    ['raycast', 'codex'],
  )
  for (let i = 0; i < 6; i++) {
    rememberRecentAppScan({ appId: `app-${i}`, appName: `App ${i}`, auditId: AUDIT })
  }
  assert.equal(loadRecentAppScans().length, 5)
  assert.equal(
    loadRecentAppScans().some((item) => item.appId === 'raycast'),
    false,
  )
})

test('malformed stored entries are dropped, never thrown', () => {
  const memory = installStorage()
  memory.store['spacelens.recentAppScans'] = JSON.stringify([
    { appId: 'good', appName: 'Good', auditId: AUDIT, lastScannedAt: '2026-10-13T00:00:00.000Z' },
    { appId: '', appName: 'Bad', auditId: AUDIT, lastScannedAt: '2026-10-13T00:00:00.000Z' },
    { appId: 'short-audit', appName: 'Bad', auditId: 'audit_x', lastScannedAt: '2026-10-13T00:00:00.000Z' },
    'not-an-object',
    null,
  ])
  assert.deepEqual(
    loadRecentAppScans().map((item) => item.appId),
    ['good'],
  )
  memory.store['spacelens.recentAppScans'] = '{broken'
  assert.deepEqual(loadRecentAppScans(), [])
})

test('staleness marks survive reload and follow the current generation', () => {
  installStorage()
  rememberRecentAppScan({ appId: 'raycast', appName: 'Raycast', auditId: AUDIT })
  const [fresh] = loadRecentAppScans()
  assert.equal(isRecentAppStale(fresh!, AUDIT), false)
  // A re-audit minted a new generation: the pinned entry is stale.
  assert.equal(isRecentAppStale(fresh!, 'audit_ffffffffffffffffffffffff'), true)
  // Without a current report only the stored flag applies.
  assert.equal(isRecentAppStale(fresh!, null), false)
  markRecentAppStale('raycast')
  const [stale] = loadRecentAppScans()
  assert.equal(stale?.stale, true)
  assert.equal(isRecentAppStale(stale!, AUDIT), true)
  assert.equal(isRecentAppStale(stale!, null), true)
  forgetRecentAppScan('raycast')
  assert.deepEqual(loadRecentAppScans(), [])
})
