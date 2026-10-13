import { test } from 'node:test'
import assert from 'node:assert/strict'
import { AppAuditLocationSchema, AppAuditReportSchema, TreeSliceNodeSchema } from '@space-lens/contract'
import type { AppAuditReport } from '@space-lens/contract'
import {
  appTreeSlice,
  basename,
  classificationBadge,
  classificationLabel,
  isPathCovered,
  initialsFor,
  matchesQuery,
  largestLocation,
  pruneStaged,
  purgeableTotal,
  validateStagedAgainstReport,
} from '../src/lib/app-audit.ts'
import { ensureIcon, readIcon, resetIconCache } from '../src/lib/app-icon-cache.ts'

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

const auditEntry = {
  id: 'raycast',
  name: 'Raycast',
  kind: 'app',
  source: 'catalog',
  bundleId: 'com.raycast.macos',
  appPath: '/Applications/Raycast.app',
  installedSize: 1_000,
  totalSize: 2_000,
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
    {
      path: '/Users/hk/Library/Application Support/com.raycast.macos',
      label: 'Support files',
      classification: 'support',
      purgeable: false,
      comment: null,
      size: 900,
      source: 'heuristic',
    },
  ],
} as const

test('appTreeSlice partitions the entry total across bundle and location branches', () => {
  const tree = appTreeSlice({ ...auditEntry, locations: [...auditEntry.locations] })
  assert.equal(tree.id, 'approot:raycast')
  assert.equal(tree.name, 'Raycast')
  assert.equal(tree.depth, 0)
  assert.equal(tree.size, 2_000)
  assert.equal(tree.hasChildren, true)
  assert.equal(tree.childCount, 3)
  const childrenTotal = tree.children.reduce((total, child) => total + child.size, 0)
  assert.equal(childrenTotal, tree.size)
  // The .app bundle is a first-class segment.
  const bundle = tree.children[0]
  assert.equal(bundle?.name, 'App bundle')
  assert.equal(bundle?.size, 1_000)
  assert.equal(bundle?.depth, 1)
  assert.equal(bundle?.hasChildren, false)
  // One leaf branch per audited location, names from label ?? basename.
  const [cache, support] = tree.children.slice(1)
  assert.equal(cache?.id, 'apploc:/Users/hk/Library/Caches/com.raycast.macos')
  assert.equal(cache?.name, 'com.raycast.macos')
  assert.equal(cache?.children.length, 0)
  assert.equal(support?.id, 'apploc:/Users/hk/Library/Application Support/com.raycast.macos')
  assert.equal(support?.name, 'Support files')
  assert.equal(TreeSliceNodeSchema.safeParse(tree).success, true)
})

test('appTreeSlice omits the bundle segment for CLI entries without an installed size', () => {
  const tree = appTreeSlice({
    ...auditEntry,
    id: 'codex-cli',
    name: 'Codex CLI',
    kind: 'cli',
    appPath: null,
    installedSize: 0,
    totalSize: 1_000,
    locations: [...auditEntry.locations],
  })
  assert.equal(tree.children.length, 2)
  assert.ok(tree.children.every((child) => child.id.startsWith('apploc:')))
  assert.equal(
    tree.children.reduce((total, child) => total + child.size, 0),
    1_000,
  )
  assert.equal(TreeSliceNodeSchema.safeParse(tree).success, true)
})

test('appTreeSlice marks an entry without children as childless but schema-valid', () => {
  const tree = appTreeSlice({
    ...auditEntry,
    appPath: null,
    installedSize: 0,
    totalSize: 0,
    locations: [],
  })
  assert.equal(tree.hasChildren, false)
  assert.equal(tree.childCount, 0)
  assert.equal(TreeSliceNodeSchema.safeParse(tree).success, true)
})

test('basename tolerates trailing slashes and bare names', () => {
  assert.equal(basename('/Users/hk/Library/Caches/com.example'), 'com.example')
  assert.equal(basename('/Users/hk/Library/Caches/com.example/'), 'com.example')
  assert.equal(basename('Code'), 'Code')
  assert.equal(basename('/'), '/')
})

test('isPathCovered only fires for a strictly staged ancestor', () => {
  const staged = new Set(['/Users/hk/Library/Application Support/com.example'])
  assert.equal(isPathCovered('/Users/hk/Library/Application Support/com.example', staged), false)
  assert.equal(
    isPathCovered('/Users/hk/Library/Application Support/com.example/clipboard', staged),
    true,
  )
  assert.equal(isPathCovered('/Users/hk/Library/Caches/com.example', staged), false)
  // A shared string prefix is not a parent folder.
  assert.equal(isPathCovered('/Users/hk/Library/Application Support/com.example-2', staged), false)
  assert.equal(isPathCovered('/Users/hk/Library/Caches/x', new Set()), false)
})

test('pruneStaged keeps only paths the latest report still lists', () => {
  assert.deepEqual(
    pruneStaged(['/a', '/b', '/c'], ['/b', '/c', '/d']),
    ['/b', '/c'],
  )
  assert.deepEqual(pruneStaged([], ['/a']), [])
})

test('validateStagedAgainstReport drops vanished apps and paths, keeps survivors', () => {
  const report: AppAuditReport = {
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
        installedSize: 0,
        totalSize: 100,
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
  const staged = new Map([
    ['raycast', new Set(['/Users/hk/Library/Caches/com.raycast.macos', '/gone/path'])],
    ['vanished-app', new Set(['/somewhere'])],
  ])
  const validated = validateStagedAgainstReport(staged, report)
  assert.deepEqual([...validated.keys()], ['raycast'])
  assert.deepEqual([...(validated.get('raycast') ?? [])], ['/Users/hk/Library/Caches/com.raycast.macos'])
  // A null report preserves the map; an empty report clears it.
  assert.deepEqual(validateStagedAgainstReport(staged, null), staged)
  assert.equal(validateStagedAgainstReport(staged, { ...report, apps: [] }).size, 0)
})
const settleIcons = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 25))

test('icon cache fetches once per path and never re-enqueues a failure', async () => {
  resetIconCache()
  let calls = 0
  const fetcher = async (path: string): Promise<string | null> => {
    calls += 1
    assert.equal(path, '/Applications/Good.app')
    return 'data:image/png;base64,AAA'
  }
  ensureIcon('/Applications/Good.app', fetcher)
  ensureIcon('/Applications/Good.app', fetcher)
  await settleIcons()
  assert.equal(calls, 1)
  assert.equal(readIcon('/Applications/Good.app')?.status, 'ready')

  resetIconCache()
  let failures = 0
  const failing = async (): Promise<string | null> => {
    failures += 1
    return null
  }
  ensureIcon('/Applications/Bad.app', failing)
  await settleIcons()
  assert.equal(failures, 1)
  assert.equal(readIcon('/Applications/Bad.app')?.status, 'failed')
  ensureIcon('/Applications/Bad.app', failing)
  await settleIcons()
  assert.equal(failures, 1)
})
