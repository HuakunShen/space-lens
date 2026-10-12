import { test } from 'node:test'
import assert from 'node:assert/strict'
import { AppAuditLocationSchema, AppAuditReportSchema } from '@space-lens/contract'
import { classificationBadge, classificationLabel, purgeableTotal, initialsFor, matchesQuery, largestLocation } from '../src/lib/app-audit.ts'

const CLASSIFICATIONS = [
  'application',
  'cache',
  'versions',
  'logs',
  're-downloadable',
  'user-data',
  'support',
] as const

test('every classification has a stable badge token and a display label', () => {
  const badges = CLASSIFICATIONS.map(classificationBadge)
  assert.equal(new Set(badges).size, CLASSIFICATIONS.length)
  for (const token of badges) {
    assert.match(token, /^app-audit-badge-[a-z-]+$/)
  }
  assert.deepEqual(CLASSIFICATIONS.map(classificationLabel), [
    'Application',
    'Cache',
    'Old versions',
    'Logs',
    'Re-downloadable',
    'User data',
    'Support',
  ])
})

test('purgeableTotal sums only purgeable locations', () => {
  assert.equal(
    purgeableTotal([
      { purgeable: true, size: 100 },
      { purgeable: false, size: 10_000 },
      { purgeable: true, size: 23 },
    ]),
    123,
  )
  assert.equal(purgeableTotal([]), 0)
})

test('initialsFor returns the first letter uppercased, with a fallback', () => {
  assert.equal(initialsFor('Raycast'), 'R')
  assert.equal(initialsFor('visual studio code'), 'V')
  assert.equal(initialsFor('  fd  '), 'F')
  assert.equal(initialsFor('1Password'), '1')
  assert.equal(initialsFor(''), '?')
  assert.equal(initialsFor('   '), '?')
})

test('matchesQuery filters by name or bundle id, case-insensitively', () => {
  const entry = { name: 'Raycast', bundleId: 'com.raycast.macos' }
  assert.equal(matchesQuery(entry, ''), true)
  assert.equal(matchesQuery(entry, '   '), true)
  assert.equal(matchesQuery(entry, 'ray'), true)
  assert.equal(matchesQuery(entry, 'RAYCAST'), true)
  assert.equal(matchesQuery(entry, 'raycast.macos'), true)
  assert.equal(matchesQuery(entry, 'COM.RAYCAST'), true)
  assert.equal(matchesQuery(entry, 'spotify'), false)
  assert.equal(matchesQuery({ name: 'ripgrep', bundleId: null }, 'rip'), true)
  assert.equal(matchesQuery({ name: 'ripgrep', bundleId: null }, 'com.'), false)
})

test('largestLocation returns the biggest location or null', () => {
  assert.equal(largestLocation({ locations: [] }), null)
  const cache = {
    path: '/Users/hk/Library/Caches/com.example',
    label: 'Cache',
    classification: 'cache',
    purgeable: true,
    comment: null,
    size: 100,
    source: 'heuristic',
  } as const
  const support = {
    path: '/Users/hk/Library/Application Support/com.example',
    label: 'Support',
    classification: 'support',
    purgeable: false,
    comment: null,
    size: 900,
    source: 'heuristic',
  } as const
  assert.equal(largestLocation({ locations: [cache] }), cache)
  assert.equal(largestLocation({ locations: [cache, support] }), support)
  assert.equal(largestLocation({ locations: [support, cache] }), support)
  // ties keep the first location
  assert.equal(largestLocation({ locations: [support, { ...support, path: '/other' }] }), support)
})

test('a malformed classification string is rejected by the contract schema', () => {
  const location = {
    path: '/Users/hk/Library/Caches/com.example',
    label: null,
    classification: 'temporary-files',
    purgeable: true,
    comment: null,
    size: 42,
    source: 'heuristic',
  }
  assert.equal(AppAuditLocationSchema.safeParse(location).success, false)
  assert.equal(
    AppAuditLocationSchema.safeParse({ ...location, classification: 'cache' }).success,
    true,
  )
})

test('a serialized sample report passes the contract report schema', () => {
  const report = {
    platform: 'macos',
    generatedAt: new Date().toISOString(),
    apps: [
      {
        id: 'raycast',
        name: 'Raycast',
        kind: 'app',
        source: 'catalog',
        bundleId: 'com.raycast.macos',
        appPath: '/Applications/Raycast.app',
        installedSize: 1_000,
        totalSize: 1_100,
        locations: [
          {
            path: '/Users/hk/Library/Caches/com.raycast.macos',
            label: null,
            classification: 'cache',
            purgeable: true,
            comment: null,
            size: 100,
            source: 'heuristic',
          },
        ],
      },
    ],
    totalApps: 1,
    truncated: false,
  }
  assert.equal(AppAuditReportSchema.safeParse(report).success, true)
  assert.equal(purgeableTotal(report.apps[0].locations), 100)
})
