// Unit tests for the Memory view's process tree: parent links, orphans,
// cycles, ordering, search and flattening.
import assert from 'node:assert/strict'
import { test } from 'node:test'
import type { MemorySnapshot, ProcessRow } from '@space-lens/contract'
import {
  ancestryOf,
  buildForest,
  buildGroupTree,
  flattenForest,
  groupAncestry,
  searchForest,
  withAncestors,
  type TreeNode,
} from '../src/lib/memory/tree.ts'

function row(overrides: Partial<ProcessRow> & { pid: number; name: string }): ProcessRow {
  return {
    ppid: null,
    startTimeUs: overrides.pid * 1_000_000,
    groupId: `grp_${overrides.pid.toString(16).padStart(16, '0')}`,
    ownedByCurrentUser: true,
    kind: 'cli',
    cpuPercent: null,
    footprintBytes: null,
    residentBytes: null,
    virtualBytes: null,
    restricted: false,
    protection: null,
    ...overrides,
  }
}

function snapshot(processes: ProcessRow[]): MemorySnapshot {
  return {
    sequence: 1,
    sampledAt: '2026-10-18T00:00:00.000Z',
    intervalMs: 1000,
    platform: 'macos',
    breakdown: {
      totalBytes: 100,
      usedBytes: 100,
      freeBytes: 0,
      availableBytes: null,
      appBytes: null,
      wiredBytes: null,
      compressedBytes: null,
      cachedFilesBytes: null,
      swapTotalBytes: 0,
      swapUsedBytes: 0,
    },
    pressure: { level: 'normal', source: 'kernel', availablePercent: null },
    groups: [],
    processes,
    notes: [],
  }
}

function names(nodes: readonly TreeNode[]): string[] {
  return nodes.map((node) => node.name)
}

interface GroupFixture {
  groupId: string
  name: string
  mainPid: number
  ppid?: number
  footprintBytes?: number
}

/** A snapshot where every group holds one process, so the group tree is exact. */
function groupSnapshot(groups: GroupFixture[]): MemorySnapshot {
  const processes = groups.map((group) =>
    row({
      pid: group.mainPid,
      name: group.name,
      groupId: group.groupId,
      ppid: group.ppid ?? null,
      footprintBytes: group.footprintBytes ?? 100,
    }),
  )
  const processGroups = groups.map((group) => ({
    groupId: group.groupId,
    name: group.name,
    bundleId: null,
    bundlePath: null,
    kind: 'cli' as const,
    mainPid: group.mainPid,
    pids: [group.mainPid],
    cpuPercent: null,
    footprintBytes: group.footprintBytes ?? 100,
    residentBytes: 0,
    frontmost: false,
    lastFrontmostAt: null,
    startedAt: '2026-10-18T00:00:00.000Z',
    protection: null,
  }))
  return { ...snapshot(processes), groups: processGroups }
}

test('a child is linked to its parent and the root keeps depth 0', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'claude', ppid: 100 }),
    ]),
  )
  assert.deepEqual(names(forest.roots), ['launchd'])
  const root = forest.roots[0]
  assert.equal(root?.children[0]?.name, 'Claude')
  assert.equal(root?.children[0]?.children[0]?.name, 'claude')
  assert.equal(root?.children[0]?.children[0]?.depth, 2)
})

test('a parent that is not in the snapshot becomes an orphan root', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 100, name: 'java', ppid: 999 }),
      row({ pid: 1, name: 'launchd' }),
    ]),
  )
  assert.deepEqual(names(forest.roots).sort(), ['java', 'launchd'])
  assert.equal(forest.orphanCount, 1)
  assert.equal(forest.byPid.get(100)?.orphan, true)
})

test('a process that is its own parent becomes an orphan root', () => {
  const forest = buildForest(snapshot([row({ pid: 100, name: 'weird', ppid: 100 })]))
  assert.deepEqual(names(forest.roots), ['weird'])
  assert.equal(forest.orphanCount, 1)
  assert.equal(forest.byPid.get(100)?.children.length, 0)
})

test('a parent link that closes a cycle is dropped instead of hanging', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 10, name: 'a', ppid: 20 }),
      row({ pid: 20, name: 'b', ppid: 10 }),
    ]),
  )
  assert.equal(forest.cycleCount, 1)
  const reachable = forest.roots.length
  assert.equal(reachable, 1)
  // Every process is still reachable from some root.
  assert.equal(forest.byPid.size, 2)
})

test('children are ordered by footprint, then name', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 10, name: 'small', ppid: 1, footprintBytes: 100 }),
      row({ pid: 11, name: 'big', ppid: 1, footprintBytes: 900 }),
      row({ pid: 12, name: 'also-big', ppid: 1, footprintBytes: 900 }),
    ]),
  )
  assert.deepEqual(names(forest.roots[0]?.children ?? []), ['also-big', 'big', 'small'])
})

test('the ancestry of a process lists its parents root first', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'zsh', ppid: 100 }),
      row({ pid: 300, name: 'rustc', ppid: 200 }),
    ]),
  )
  assert.deepEqual(names(ancestryOf(forest, 300)), ['launchd', 'Claude', 'zsh', 'rustc'])
  assert.deepEqual(ancestryOf(forest, 9999), [])
})

test('a search keeps the matches and the ancestors that lead to them', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'zsh', ppid: 100 }),
      row({ pid: 300, name: 'rustc', ppid: 200 }),
      row({ pid: 400, name: 'node', ppid: 1 }),
    ]),
  )
  const search = searchForest(forest, 'rustc')
  assert.deepEqual([...search.matchPids], [300])
  assert.deepEqual([...search.keepPids].sort((a, b) => a - b), [1, 100, 200, 300])
})

test('a search also matches the group name and the pid', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 4242, name: 'helper', ppid: 1, groupId: 'grp_warp' }),
    ]),
  )
  assert.deepEqual([...searchForest(forest, '4242').matchPids], [4242])
})

test('flattening hides collapsed branches and expands the open ones', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'claude', ppid: 100 }),
    ]),
  )
  const collapsed = flattenForest(forest, { expanded: new Set() })
  assert.deepEqual(collapsed.map((row) => row.node.name), ['launchd'])

  const opened = flattenForest(forest, { expanded: new Set([1, 100]) })
  assert.deepEqual(opened.map((row) => row.node.name), ['launchd', 'Claude', 'claude'])
  assert.deepEqual(opened.map((row) => row.depth), [0, 1, 2])
})

test('the tree is open by default and remembers what the user closed', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'claude', ppid: 100 }),
      row({ pid: 300, name: 'rustc', ppid: 200 }),
    ]),
  )

  // htop's default: every branch open, so the first screen is the tree the
  // view exists to show.
  const open = flattenForest(forest)
  assert.deepEqual(open.map((row) => row.node.name), ['launchd', 'Claude', 'claude', 'rustc'])
  assert.ok(open.every((row) => row.expanded))

  // A collapse closes one branch and nothing else.
  const rows = flattenForest(forest, { collapsed: new Set([100]) })
  assert.deepEqual(rows.map((row) => row.node.name), ['launchd', 'Claude'])
  assert.equal(rows[1].expanded, false)
})

test('a kept chain stays open even when the user collapsed it', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'claude', ppid: 100 }),
      row({ pid: 300, name: 'rustc', ppid: 200 }),
    ]),
  )
  const keep = withAncestors(forest, new Set([300]))

  const rows = flattenForest(forest, { keep, collapsed: new Set([1, 100, 200]) })

  assert.deepEqual(rows.map((row) => row.node.name), ['launchd', 'Claude', 'claude', 'rustc'])
})

test('flattening with a keep set renders only the matching chain', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'claude', ppid: 100 }),
      row({ pid: 300, name: 'rustc', ppid: 200 }),
      row({ pid: 400, name: 'node', ppid: 1 }),
    ]),
  )
  const keep = searchForest(forest, 'rustc').keepPids
  const rows = flattenForest(forest, { keep })
  assert.deepEqual(rows.map((row) => row.node.name), ['launchd', 'Claude', 'claude', 'rustc'])
  assert.deepEqual(rows.map((row) => row.expanded), [true, true, true, true])
})

test('each row carries its ancestors so the view can say who launched it', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1 }),
      row({ pid: 200, name: 'rustc', ppid: 100 }),
    ]),
  )
  const rows = flattenForest(forest, { expanded: new Set([1, 100]) })
  const rustc = rows.find((row) => row.node.name === 'rustc')
  assert.deepEqual(names(rustc?.ancestors ?? []), ['launchd', 'Claude'])
})

test('the "mine" scope keeps the user processes and their system ancestors', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'Claude', ppid: 1, ownedByCurrentUser: true }),
      row({ pid: 200, name: 'claude', ppid: 100, ownedByCurrentUser: true }),
      row({ pid: 300, name: 'mds', ppid: 1, ownedByCurrentUser: false }),
    ]),
  )
  const owned = new Set([100, 200])
  const keep = withAncestors(forest, owned)
  assert.deepEqual([...keep].sort((a, b) => a - b), [1, 100, 200])

  // The scope keeps the tree shape: a collapsed branch still hides its
  // children, unlike the search mode, which opens the whole chain.
  const rows = flattenForest(forest, { keep, expandKept: false, expanded: new Set([1]) })
  assert.deepEqual(rows.map((row) => row.node.name), ['launchd', 'Claude'])
})

test('a search inside the "mine" scope drops the matches the user does not own', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd' }),
      row({ pid: 100, name: 'java', ppid: 1, ownedByCurrentUser: true }),
      row({ pid: 200, name: 'java', ppid: 1, ownedByCurrentUser: false }),
    ]),
  )
  const search = searchForest(forest, 'java')
  assert.equal(search.matchPids.size, 2)
  const mine = new Set([...search.matchPids].filter((pid) => pid === 100))
  const rows = flattenForest(forest, { keep: withAncestors(forest, mine) })
  assert.deepEqual(rows.map((row) => row.node.name), ['launchd', 'java'])
})

test('a collapsed branch knows how much it hides', () => {
  const forest = buildForest(
    snapshot([
      row({ pid: 1, name: 'launchd', footprintBytes: 0 }),
      row({ pid: 100, name: 'Claude', ppid: 1, footprintBytes: 100 }),
      row({ pid: 200, name: 'claude', ppid: 100, footprintBytes: 200 }),
      row({ pid: 300, name: 'rustc', ppid: 200, footprintBytes: 400 }),
    ]),
  )
  const claude = forest.byPid.get(100)
  assert.equal(claude?.descendantCount, 2)
  assert.equal(claude?.subtreeBytes, 700)
  assert.equal(forest.byPid.get(300)?.descendantCount, 0)
  assert.equal(forest.byPid.get(300)?.subtreeBytes, 400)
})

test('the group tree names the group that launched each group', () => {
  const snap = groupSnapshot([
    { groupId: 'grp_a', name: 'Claude', mainPid: 100 },
    { groupId: 'grp_b', name: 'claude', mainPid: 200, ppid: 100 },
    { groupId: 'grp_c', name: 'rustc', mainPid: 300, ppid: 200 },
  ])
  const tree = buildGroupTree(snap)

  assert.equal(tree.parentOf.get('grp_a'), null)
  assert.equal(tree.parentOf.get('grp_b'), 'grp_a')
  assert.equal(tree.parentOf.get('grp_c'), 'grp_b')
  assert.deepEqual(tree.childrenOf.get('grp_a'), ['grp_b'])
  assert.deepEqual(groupAncestry(tree, 'grp_c'), ['grp_a', 'grp_b', 'grp_c'])
  assert.deepEqual(groupAncestry(tree, 'grp_unknown'), [])
})

test('the group tree orders the children by footprint', () => {
  const snap = groupSnapshot([
    { groupId: 'grp_a', name: 'Parent', mainPid: 100 },
    { groupId: 'grp_small', name: 'small', mainPid: 200, ppid: 100, footprintBytes: 10 },
    { groupId: 'grp_big', name: 'big', mainPid: 300, ppid: 100, footprintBytes: 900 },
  ])
  const tree = buildGroupTree(snap)

  assert.deepEqual(tree.childrenOf.get('grp_a'), ['grp_big', 'grp_small'])
})

test('a group whose parent is not in the snapshot is a root', () => {
  const snap = groupSnapshot([
    { groupId: 'grp_a', name: 'lonely', mainPid: 100, ppid: 999 },
  ])
  const tree = buildGroupTree(snap)

  assert.equal(tree.parentOf.get('grp_a'), null)
})
